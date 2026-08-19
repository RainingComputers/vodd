use std::collections::BTreeMap;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use vodd::bitcode;
use vodd::interpreter;
use vodd::parser;

const SLACK: usize = 4096;
const FUEL: usize = 1 << 24;

const REFERENCE_DEFECTS: &[(&str, &str)] = &[
    (
        "frem_float",
        "rusticl OpFRem returns operand 1 instead of zero when both operands are equal",
    ),
    (
        "frem_float4",
        "rusticl OpFRem returns operand 1 instead of zero when both operands are equal",
    ),
    (
        "fmod_float",
        "rusticl OpFMod returns operand 1 instead of zero when both operands are equal",
    ),
    (
        "fmod_float4",
        "rusticl OpFMod returns operand 1 instead of zero when both operands are equal",
    ),
    (
        "decorate_saturated_conversion_float_to_uchar",
        "rusticl OpConvertFToU with SaturatedConversion clamps the upper bound but wraps negatives instead of clamping to zero",
    ),
];

const BASE85: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

fn decode_base85(text: &str) -> Vec<u8> {
    let mut digits: Vec<u32> = Vec::new();
    for character in text.bytes() {
        if character.is_ascii_whitespace() {
            continue;
        }
        let position = BASE85
            .iter()
            .position(|candidate| *candidate == character)
            .unwrap_or_else(|| panic!("byte {character} is not in the base85 alphabet"));
        digits.push(position as u32);
    }

    let mut bytes = Vec::with_capacity(digits.len() * 4 / 5);
    for group in digits.chunks(5) {
        let mut accumulator: u32 = 0;
        for index in 0..5 {
            let digit = group.get(index).copied().unwrap_or(84);
            accumulator = accumulator.wrapping_mul(85).wrapping_add(digit);
        }
        let word = accumulator.to_be_bytes();
        let produced = group.len() - 1;
        bytes.extend_from_slice(&word[..produced]);
    }
    bytes
}

fn assemble(assembly: &str) -> Vec<u8> {
    let mut child = Command::new("spirv-as")
        .args(["--target-env", "opencl1.2", "-", "-o", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spirv-as must be installed and on PATH");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(assembly.as_bytes())
        .expect("write assembly");
    let output = child.wait_with_output().expect("spirv-as");
    assert!(
        output.status.success(),
        "spirv-as failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

type Buffers = Vec<(u64, Vec<u8>)>;

fn locate(buffers: &Buffers, address: u64, length: usize) -> Result<(usize, usize), String> {
    for (index, (base, bytes)) in buffers.iter().enumerate() {
        if address >= *base && address + length as u64 <= *base + bytes.len() as u64 {
            return Ok((index, (address - *base) as usize));
        }
    }

    Err(format!("address {address:#x} is not inside any buffer"))
}

fn read(buffers: &Buffers, address: u64, size: usize) -> Result<Vec<u8>, String> {
    let (index, offset) = locate(buffers, address, size)?;

    Ok(buffers[index].1[offset..offset + size].to_vec())
}

fn write(buffers: &mut Buffers, address: u64, bytes: &[u8]) -> Result<(), String> {
    let (index, offset) = locate(buffers, address, bytes.len())?;
    buffers[index].1[offset..offset + bytes.len()].copy_from_slice(bytes);

    Ok(())
}

fn driver(
    interpreter: &mut interpreter::Interpreter,
    buffers: &mut Buffers,
    global_id: [u64; 3],
) -> Result<(), String> {
    let mut reply = interpreter::Resume::Start;

    loop {
        let Some(reason) = interpreter
            .resume(reply)
            .map_err(|error| format!("{error:?}"))?
        else {
            return Ok(());
        };

        reply = match reason {
            interpreter::YieldReason::Read { address, size }
            | interpreter::YieldReason::ReadLocal { address, size } => {
                interpreter::Resume::Bytes(read(buffers, address, size)?)
            }
            interpreter::YieldReason::Write { address, bytes }
            | interpreter::YieldReason::WriteLocal { address, bytes } => {
                write(buffers, address, &bytes)?;
                interpreter::Resume::Ack
            }
            interpreter::YieldReason::Atomic { operation, address, width } => {
                let size = (width as usize).div_ceil(8);
                let mut buffer = [0u8; 8];
                buffer[..size].copy_from_slice(&read(buffers, address, size)?);

                let previous = u64::from_le_bytes(buffer);
                let updated = match operation {
                    interpreter::Atomic::Increment => previous.wrapping_add(1),
                    interpreter::Atomic::Decrement => previous.wrapping_sub(1),
                };

                write(buffers, address, &updated.to_le_bytes()[..size])?;
                interpreter::Resume::Scalar(previous)
            }
            interpreter::YieldReason::Builtin(builtin) => {
                interpreter::Resume::Builtin(match builtin {
                    bitcode::Builtin::GlobalInvocationId => global_id,
                    bitcode::Builtin::NumWorkgroups => [1, 1, 1],
                    bitcode::Builtin::WorkgroupSize => [1, 1, 1],
                    _ => [0, 0, 0],
                })
            }
            interpreter::YieldReason::MemoryBarrier { .. }
            | interpreter::YieldReason::ControlBarrier { .. } => interpreter::Resume::Ack,
        };
    }
}

struct Case {
    module: String,
    entry: String,
    global_work_size: usize,
    out_arg: usize,
    compare: String,
    assembly: String,
    expected: Vec<u8>,
    arguments: Vec<CaseArgument>,
}

struct CaseArgument {
    kind: String,
    bytes: Vec<u8>,
}

fn load_corpus() -> Vec<Case> {
    let text = std::fs::read_to_string("conformance/cases.yaml").expect("conformance/cases.yaml");
    let documents = yaml_rust2::YamlLoader::load_from_str(&text).expect("parse cases.yaml");
    let root = &documents[0];
    let entries = root["cases"].as_vec().expect("cases list");

    let mut cases = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut arguments = Vec::new();
        for argument in entry["args"].as_vec().expect("args") {
            let kind = argument["kind"].as_str().expect("kind").to_string();
            let encoded = if kind == "buffer" {
                argument["data"].as_str().expect("data")
            } else {
                argument["value"].as_str().expect("value")
            };
            arguments.push(CaseArgument { kind, bytes: decode_base85(encoded) });
        }
        cases.push(Case {
            module: entry["module"].as_str().expect("module").to_string(),
            entry: entry["entry"].as_str().expect("entry").to_string(),
            global_work_size: entry["global_work_size"].as_i64().expect("size") as usize,
            out_arg: entry["out_arg"].as_i64().expect("out_arg") as usize,
            compare: entry["compare"].as_str().expect("compare").to_string(),
            assembly: entry["spirv_assembly"]
                .as_str()
                .expect("assembly")
                .to_string(),
            expected: decode_base85(entry["expected"].as_str().expect("expected")),
            arguments,
        });
    }
    cases
}

fn run_case(case: &Case) -> Result<Vec<u8>, String> {
    let binary = assemble(&case.assembly);
    let module = parser::parse(&binary).map_err(|error| format!("parse: {error:?}"))?;
    let function = module
        .entry(&case.entry)
        .map_err(|error| format!("entry: {error:?}"))?
        .result;

    let mut buffers: Buffers = Vec::new();
    let mut arguments = Vec::new();
    let mut next_base = 0x1000u64;
    for argument in &case.arguments {
        if argument.kind == "buffer" {
            let base = next_base;
            let mut backing = argument.bytes.clone();
            backing.resize(argument.bytes.len() + SLACK, 0);
            next_base += (backing.len() as u64 + 0x1000) & !0xFFF;
            buffers.push((base, backing));
            arguments.push(interpreter::Argument::Buffer(base));
        } else {
            arguments.push(interpreter::Argument::Value(argument.bytes.clone()));
        }
    }

    for id in 0..case.global_work_size {
        let mut interpreter =
            interpreter::Interpreter::new(module.clone(), function, &arguments, FUEL)
                .map_err(|error| format!("invocation: {error:?}"))?;

        driver(&mut interpreter, &mut buffers, [id as u64, 0, 0])
            .map_err(|error| format!("work item {id}: {error}"))?;
    }

    let mut buffer_index = 0;
    for (index, argument) in case.arguments.iter().enumerate() {
        if argument.kind != "buffer" {
            continue;
        }
        if index == case.out_arg {
            let produced = &buffers[buffer_index].1;
            return Ok(produced[..produced.len() - SLACK].to_vec());
        }
        buffer_index += 1;
    }
    Err("output argument is not a buffer".to_string())
}

fn ulp_distance(one: u32, other: u32) -> u64 {
    let order = |bits: u32| -> i64 {
        if bits & 0x8000_0000 != 0 {
            -((bits & 0x7FFF_FFFF) as i64)
        } else {
            bits as i64
        }
    };
    (order(one) - order(other)).unsigned_abs()
}

fn compare(case: &Case, produced: &[u8]) -> Result<(), String> {
    match case.compare.as_str() {
        "run_only" => Ok(()),
        "exact" => {
            if produced == case.expected {
                Ok(())
            } else {
                let position = produced
                    .iter()
                    .zip(&case.expected)
                    .position(|(one, other)| one != other)
                    .unwrap_or(0);
                Err(format!(
                    "first difference at byte {position}: produced {:02x?} expected {:02x?}",
                    &produced[position..(position + 4).min(produced.len())],
                    &case.expected[position..(position + 4).min(case.expected.len())]
                ))
            }
        }
        "ulp" => {
            for (index, (one, other)) in produced
                .chunks_exact(4)
                .zip(case.expected.chunks_exact(4))
                .enumerate()
            {
                let produced_bits = u32::from_le_bytes([one[0], one[1], one[2], one[3]]);
                let expected_bits = u32::from_le_bytes([other[0], other[1], other[2], other[3]]);
                if produced_bits == expected_bits {
                    continue;
                }
                if ulp_distance(produced_bits, expected_bits) > 2 {
                    return Err(format!(
                        "lane {index} differs by more than 2 ULP: {produced_bits:08x} vs {expected_bits:08x}"
                    ));
                }
            }
            Ok(())
        }
        "permutation" => {
            let mut produced_values: Vec<u32> = produced
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect();
            let mut expected_values: Vec<u32> = case
                .expected
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect();
            produced_values.sort_unstable();
            expected_values.sort_unstable();
            if produced_values == expected_values {
                Ok(())
            } else {
                Err("sorted contents differ".to_string())
            }
        }
        other => Err(format!("unknown compare mode {other}")),
    }
}

#[test]
fn corpus() {
    let cases = load_corpus();
    assert!(!cases.is_empty(), "corpus is empty");

    let filter = std::env::var("VODD_CASE").ok();
    let mut passed = Vec::new();
    let mut diverged = Vec::new();
    let mut failed: BTreeMap<String, String> = BTreeMap::new();

    for case in &cases {
        if let Some(wanted) = &filter
            && &case.module != wanted
        {
            continue;
        }
        let defect = REFERENCE_DEFECTS
            .iter()
            .find(|(module, _)| *module == case.module);
        let outcome = run_case(case).and_then(|produced| compare(case, &produced));
        match (outcome, defect) {
            (Ok(()), None) => passed.push(case.module.clone()),
            (Ok(()), Some((module, _))) => {
                failed.insert(
                    (*module).to_string(),
                    "expected to diverge from the reference but matched it".to_string(),
                );
            }
            (Err(_), Some((module, reason))) => diverged.push((*module, *reason)),
            (Err(reason), None) => {
                failed.insert(case.module.clone(), reason);
            }
        }
    }

    println!(
        "conformance: {} passed, {} known reference defects, {} failed, {} total",
        passed.len(),
        diverged.len(),
        failed.len(),
        passed.len() + diverged.len() + failed.len()
    );
    for (module, reason) in &diverged {
        println!("  DIVERGES {module}: {reason}");
    }
    for (module, reason) in &failed {
        println!("  FAIL {module}: {reason}");
    }
    assert!(failed.is_empty(), "{} cases failed", failed.len());
}

#[test]
fn barriers_reach_the_driver() {
    let case = load_corpus()
        .into_iter()
        .find(|case| case.module == "memory_barrier")
        .expect("the corpus must contain the memory_barrier case");

    let binary = assemble(&case.assembly);
    let module = parser::parse(&binary).expect("parse");
    let function = module.entry(&case.entry).expect("entry").result;

    let arguments = [
        interpreter::Argument::Buffer(0x1000),
        interpreter::Argument::Buffer(0x2000),
    ];
    let mut interpreter =
        interpreter::Interpreter::new(module, function, &arguments, FUEL).expect("invocation");

    let mut buffers: Buffers = vec![
        (0x1000, vec![0; 1024]),
        (0x2000, case.arguments[1].bytes.clone()),
    ];
    let mut barriers = Vec::new();
    let mut reply = interpreter::Resume::Start;

    while let Some(reason) = interpreter.resume(reply).expect("resume") {
        reply = match reason {
            interpreter::YieldReason::MemoryBarrier { .. }
            | interpreter::YieldReason::ControlBarrier { .. } => {
                barriers.push(reason);
                interpreter::Resume::Ack
            }
            interpreter::YieldReason::Read { address, size } => {
                interpreter::Resume::Bytes(read(&buffers, address, size).expect("read"))
            }
            interpreter::YieldReason::Write { address, bytes } => {
                write(&mut buffers, address, &bytes).expect("write");
                interpreter::Resume::Ack
            }
            interpreter::YieldReason::Builtin(_) => interpreter::Resume::Builtin([7, 0, 0]),
            other => panic!("unexpected yield {other:?}"),
        };
    }

    assert_eq!(
        barriers,
        vec![
            interpreter::YieldReason::MemoryBarrier { semantics: 528 },
            interpreter::YieldReason::ControlBarrier { execution_scope: 2, semantics: 272 },
        ]
    );
}
