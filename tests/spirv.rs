use std::io::Write;
use std::process::Command;
use std::process::Stdio;

mod common;

use common::integer_field;
use common::string_field;

use vodd::address;
use vodd::bitcode;
use vodd::detectors;
use vodd::interpreter;
use vodd::parser;

const ASSEMBLER: &str = "spirv-as";
const FUEL: usize = 1 << 24;

type Buffers = address::Storage;

type Driver<'a> =
    Box<dyn Fn(&mut interpreter::Interpreter, &mut Buffers, [u64; 3]) -> Result<(), String> + 'a>;

struct SpirvCase {
    module: String,
    entry: String,
    global_work_size: usize,
    out_arg: usize,
    compare: String,
    assembly: String,
    expected: Vec<u8>,
    expected_yields: Vec<interpreter::YieldReason>,
    arguments: Vec<SpirvCaseArgument>,
}

struct SpirvCaseArgument {
    kind: String,
    bytes: Vec<u8>,
}

enum SpirvCaseResult {
    Passed,
    Failed { module: String, reason: String },
}

impl SpirvCaseResult {
    fn failure(&self) -> Option<(&str, &str)> {
        match self {
            SpirvCaseResult::Passed => None,
            SpirvCaseResult::Failed { module, reason } => Some((module, reason)),
        }
    }
}

#[test]
fn spirv_cases() {
    let spirv_cases = load_spirv_cases();
    assert!(!spirv_cases.is_empty(), "no SPIR-V cases loaded");

    find_assembler().unwrap_or_else(|error| panic!("{error}"));

    let filter = std::env::var("VODD_SPIRV_CASE").ok();

    let results: Vec<SpirvCaseResult> = spirv_cases
        .iter()
        .filter(|spirv_case| {
            filter
                .as_ref()
                .is_none_or(|wanted| &spirv_case.module == wanted)
        })
        .map(|spirv_case| {
            let drive: Driver = if spirv_case.expected_yields.is_empty() {
                Box::new(driver)
            } else {
                Box::new(|interpreter, buffers, global_id| {
                    driver_with_yields_expect(
                        interpreter,
                        buffers,
                        global_id,
                        &spirv_case.expected_yields,
                    )
                })
            };

            match run_spirv_case(spirv_case, drive)
                .and_then(|produced| compare(spirv_case, &produced))
            {
                Ok(()) => SpirvCaseResult::Passed,
                Err(reason) => {
                    SpirvCaseResult::Failed { module: spirv_case.module.clone(), reason }
                }
            }
        })
        .collect();

    let failures: Vec<(&str, &str)> = results
        .iter()
        .filter_map(SpirvCaseResult::failure)
        .collect();

    println!(
        "conformance: {} passed, {} failed, {} total",
        results.len() - failures.len(),
        failures.len(),
        results.len()
    );

    for (module, reason) in &failures {
        println!("  FAIL {module}: {reason}");
    }

    assert!(
        failures.is_empty(),
        "{} SPIR-V cases failed",
        failures.len()
    );
}

fn run_spirv_case(spirv_case: &SpirvCase, drive: Driver<'_>) -> Result<Vec<u8>, String> {
    let binary = assemble(&spirv_case.assembly)?;
    let module =
        std::sync::Arc::new(parser::parse(&binary).map_err(|error| format!("parse: {error:?}"))?);
    let function = module
        .entry(&spirv_case.entry)
        .map_err(|error| format!("entry: {error:?}"))?
        .result;

    let (mut buffers, arguments) = create_buffer(&spirv_case.arguments);

    for id in 0..spirv_case.global_work_size {
        let mut interpreter = interpreter::Interpreter::new(
            std::sync::Arc::clone(&module),
            function,
            &arguments,
            interpreter::local_layout(&module, function)
                .map_err(|error| format!("local memory: {error:?}"))?
                .0,
            FUEL,
            false,
            detectors::Checks::from_env().expect("VODD_CHECK"),
        )
        .map_err(|error| format!("invocation: {error:?}"))?;

        drive(&mut interpreter, &mut buffers, [id as u64, 0, 0])
            .map_err(|error| format!("work item {id}: {error}"))?;
    }

    let Some((&interpreter::Argument::Buffer(address), output)) = arguments
        .get(spirv_case.out_arg)
        .zip(spirv_case.arguments.get(spirv_case.out_arg))
    else {
        return Err("output argument is not a buffer".to_string());
    };

    read(&buffers, address, output.bytes.len())
}

fn driver_with_yields_expect(
    interpreter: &mut interpreter::Interpreter,
    buffers: &mut Buffers,
    global_id: [u64; 3],
    expected: &[interpreter::YieldReason],
) -> Result<(), String> {
    let watched: Vec<_> = expected.iter().map(std::mem::discriminant).collect();

    let mut reply = interpreter::Resume::Start;
    let mut yields = Vec::new();

    while let Some(reason) = interpreter
        .resume(reply)
        .map_err(|error| format!("resume: {error:?}"))?
    {
        if watched.contains(&std::mem::discriminant(&reason)) {
            yields.push(reason.clone());
        }

        reply = driver_inner(reason, buffers, global_id)?;
    }

    if yields != expected {
        return Err(format!(
            "expected yields {expected:?} but reached {yields:?}"
        ));
    }

    Ok(())
}

fn driver(
    interpreter: &mut interpreter::Interpreter,
    buffers: &mut Buffers,
    global_id: [u64; 3],
) -> Result<(), String> {
    let mut reply = interpreter::Resume::Start;

    while let Some(reason) = interpreter
        .resume(reply)
        .map_err(|error| format!("resume: {error:?}"))?
    {
        reply = driver_inner(reason, buffers, global_id)?;
    }

    Ok(())
}

fn driver_inner(
    reason: interpreter::YieldReason,
    buffers: &mut Buffers,
    global_id: [u64; 3],
) -> Result<interpreter::Resume, String> {
    Ok(match reason {
        interpreter::YieldReason::Read { address, size }
        | interpreter::YieldReason::ReadLocal { address, size } => {
            interpreter::Resume::Bytes(read(buffers, address, size)?)
        }
        interpreter::YieldReason::Write { address, bytes }
        | interpreter::YieldReason::WriteLocal { address, bytes } => {
            write(buffers, address, &bytes)?;
            interpreter::Resume::Ack
        }
        interpreter::YieldReason::Atomic {
            operation, address, width, value, comparator, ..
        } => {
            let size = (width as usize).div_ceil(8);
            let mut buffer = [0u8; 8];

            buffer[..size].copy_from_slice(&read(buffers, address, size)?);

            let previous = u64::from_le_bytes(buffer);
            let updated = atomic(operation, previous, value, comparator, width);

            write(buffers, address, &updated.to_le_bytes()[..size])?;

            interpreter::Resume::Scalar(previous)
        }
        interpreter::YieldReason::Builtin(builtin) => interpreter::Resume::Builtin(match builtin {
            bitcode::Builtin::GlobalInvocationId => global_id,
            bitcode::Builtin::NumWorkgroups => [1, 1, 1],
            bitcode::Builtin::WorkgroupSize => [1, 1, 1],
            _ => [0, 0, 0],
        }),
        interpreter::YieldReason::Diagnostic(diagnostics) => {
            let reported: Vec<String> = diagnostics
                .into_iter()
                .map(|diagnostic| format!("{diagnostic}"))
                .collect();

            return Err(format!("diagnostics: {}", reported.join("; ")));
        }
        interpreter::YieldReason::MemoryBarrier { .. }
        | interpreter::YieldReason::ControlBarrier { .. }
        | interpreter::YieldReason::Break => interpreter::Resume::Ack,
    })
}

fn atomic(
    operation: interpreter::Atomic,
    previous: u64,
    value: u64,
    comparator: u64,
    width: u32,
) -> u64 {
    let signed = |bits: u64| ((bits << (64 - width)) as i64) >> (64 - width);

    match operation {
        interpreter::Atomic::Load => previous,
        interpreter::Atomic::Store | interpreter::Atomic::Exchange => value,
        interpreter::Atomic::CompareExchange => {
            if previous == comparator {
                value
            } else {
                previous
            }
        }
        interpreter::Atomic::Increment => previous.wrapping_add(1),
        interpreter::Atomic::Decrement => previous.wrapping_sub(1),
        interpreter::Atomic::Add => previous.wrapping_add(value),
        interpreter::Atomic::Sub => previous.wrapping_sub(value),
        interpreter::Atomic::SignedMin => signed(previous).min(signed(value)) as u64,
        interpreter::Atomic::UnsignedMin => previous.min(value),
        interpreter::Atomic::SignedMax => signed(previous).max(signed(value)) as u64,
        interpreter::Atomic::UnsignedMax => previous.max(value),
        interpreter::Atomic::And => previous & value,
        interpreter::Atomic::Or => previous | value,
        interpreter::Atomic::Xor => previous ^ value,
    }
}

fn create_buffer(case_arguments: &[SpirvCaseArgument]) -> (Buffers, Vec<interpreter::Argument>) {
    let mut buffers = Buffers::new(address::Region::Global);
    let mut arguments = Vec::new();

    for argument in case_arguments {
        if argument.kind != "buffer" {
            arguments.push(interpreter::Argument::Value(argument.bytes.clone()));

            continue;
        }

        let size = argument.bytes.len();
        let address = buffers.allocate(size, 1).expect("global slots");

        buffers
            .write(address, size)
            .expect("global slot")
            .copy_from_slice(&argument.bytes);

        arguments.push(interpreter::Argument::Buffer(address));
    }

    (buffers, arguments)
}

fn read(buffers: &Buffers, address: u64, size: usize) -> Result<Vec<u8>, String> {
    buffers
        .read(address, size)
        .map(<[u8]>::to_vec)
        .map_err(|fault| format!("address {address:#x}: {fault:?}"))
}

fn write(buffers: &mut Buffers, address: u64, bytes: &[u8]) -> Result<(), String> {
    buffers
        .write(address, bytes.len())
        .map_err(|fault| format!("address {address:#x}: {fault:?}"))?
        .copy_from_slice(bytes);

    Ok(())
}

fn load_spirv_cases() -> Vec<SpirvCase> {
    let path = "tests/data/spirv.yaml";
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let documents = yaml_rust2::YamlLoader::load_from_str(&text)
        .unwrap_or_else(|error| panic!("{path} is not valid YAML: {error}"));
    let root = &documents[0];

    root["spirv_cases"]
        .as_vec()
        .unwrap_or_else(|| panic!("{path} has no top level spirv_cases list"))
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let module = entry["module"]
                .as_str()
                .unwrap_or_else(|| panic!("{path}: SPIR-V case {index} has no module name"))
                .to_string();
            let context = format!("SPIR-V case {module}");

            SpirvCase {
                entry: string_field(entry, "entry", &context).to_string(),
                global_work_size: integer_field(entry, "global_work_size", &context),
                out_arg: integer_field(entry, "out_arg", &context),
                compare: string_field(entry, "compare", &context).to_string(),
                assembly: string_field(entry, "spirv_assembly", &context).to_string(),
                expected: decode_base85(string_field(entry, "expected", &context)),
                expected_yields: entry["expected_yields"]
                    .as_vec()
                    .map(|yields| {
                        yields
                            .iter()
                            .enumerate()
                            .map(|(index, expected)| {
                                yield_reason(expected, &format!("{context} yield {index}"))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                arguments: entry["args"]
                    .as_vec()
                    .unwrap_or_else(|| panic!("{context}: args is missing or is not a list"))
                    .iter()
                    .enumerate()
                    .map(|(index, argument)| {
                        let context = format!("{context} argument {index}");
                        let kind = string_field(argument, "kind", &context).to_string();
                        let name = if kind == "buffer" { "data" } else { "value" };

                        SpirvCaseArgument {
                            bytes: decode_base85(string_field(argument, name, &context)),
                            kind,
                        }
                    })
                    .collect(),
                module,
            }
        })
        .collect()
}

fn yield_reason(entry: &yaml_rust2::Yaml, context: &str) -> interpreter::YieldReason {
    let kind = string_field(entry, "kind", context);

    match kind {
        "memory_barrier" => interpreter::YieldReason::MemoryBarrier {
            semantics: bitcode::MemorySemantics::from_word(integer_field(
                entry,
                "semantics",
                context,
            ) as u32)
            .expect("memory semantics"),
        },
        "control_barrier" => interpreter::YieldReason::ControlBarrier {
            execution_scope: integer_field(entry, "execution_scope", context) as u64,
            semantics: bitcode::MemorySemantics::from_word(integer_field(
                entry,
                "semantics",
                context,
            ) as u32)
            .expect("memory semantics"),
        },
        other => panic!("{context}: unknown expected yield kind {other}"),
    }
}

fn compare(spirv_case: &SpirvCase, produced: &[u8]) -> Result<(), String> {
    if produced.len() != spirv_case.expected.len() {
        return Err(format!(
            "produced {} bytes but expected {}",
            produced.len(),
            spirv_case.expected.len()
        ));
    }

    match spirv_case.compare.as_str() {
        "run_only" => Ok(()),
        "exact" => {
            if produced == spirv_case.expected {
                Ok(())
            } else {
                let position = produced
                    .iter()
                    .zip(&spirv_case.expected)
                    .position(|(one, other)| one != other)
                    .unwrap_or(0);

                Err(format!(
                    "first difference at byte {position}: produced {:02x?} expected {:02x?}",
                    &produced[position..(position + 4).min(produced.len())],
                    &spirv_case.expected[position..(position + 4).min(spirv_case.expected.len())]
                ))
            }
        }
        "ulp" => {
            for (index, (one, other)) in words(produced)?
                .iter()
                .zip(words(&spirv_case.expected)?)
                .enumerate()
            {
                let produced_bits = u32::from_le_bytes(*one);
                let expected_bits = u32::from_le_bytes(*other);

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
            let mut produced_values: Vec<u32> = words(produced)?
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect();
            let mut expected_values: Vec<u32> = words(&spirv_case.expected)?
                .iter()
                .map(|word| u32::from_le_bytes(*word))
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

fn words(bytes: &[u8]) -> Result<&[[u8; 4]], String> {
    match bytes.as_chunks::<4>() {
        (words, []) => Ok(words),
        (_, rest) => Err(format!("{} trailing bytes do not make a word", rest.len())),
    }
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

fn find_assembler() -> Result<(), String> {
    Command::new(ASSEMBLER)
        .arg("--version")
        .output()
        .map(|_| ())
        .map_err(|error| format!("{ASSEMBLER} must be installed and on PATH: {error}"))
}

fn assemble(assembly: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new(ASSEMBLER)
        .args(["--target-env", "opencl1.2", "-", "-o", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("assemble: spawning {ASSEMBLER}: {error}"))?;

    let written = child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(assembly.as_bytes());

    let output = child
        .wait_with_output()
        .map_err(|error| format!("assemble: waiting for spirv-as: {error}"))?;

    written.map_err(|error| format!("assemble: writing to spirv-as: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "assemble: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(output.stdout)
}

fn decode_base85(text: &str) -> Vec<u8> {
    const BASE85: &[u8] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

    let digits: Vec<u32> = text
        .bytes()
        .filter(|character| !character.is_ascii_whitespace())
        .map(|character| {
            BASE85
                .iter()
                .position(|candidate| *candidate == character)
                .unwrap_or_else(|| panic!("byte {character} is not in the base85 alphabet"))
                as u32
        })
        .collect();

    digits
        .chunks(5)
        .flat_map(|group| {
            let accumulator = group
                .iter()
                .copied()
                .chain(std::iter::repeat(84))
                .take(5)
                .fold(0u32, |accumulator, digit| {
                    accumulator.wrapping_mul(85).wrapping_add(digit)
                });

            accumulator.to_be_bytes().into_iter().take(group.len() - 1)
        })
        .collect()
}
