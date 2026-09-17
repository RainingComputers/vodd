mod common;

use common::HEADERS;
use common::REDIRECT;
use common::VODD;
use common::build_driver;
use common::documents;
use common::is_current;
use common::link_arguments;
use common::run_with_timeout;
use common::runtime_environment;

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const TIMEOUT: u64 = 60;
const HOST: &str = "tests/support/host.c";
const ARTIFACTS: &str = "target/oclgrind";
const TABLES: &[&str] = &[
    "tests/data/kernels/oclgrind.yaml",
    "tests/data/kernels/vodd.yaml",
];

type Counts = BTreeMap<String, usize>;

struct Case {
    name: String,
    origin: String,
    entry: String,
    global_size: String,
    local_size: String,
    arguments: Vec<String>,
    local_memory: Option<i64>,
    expected: Counts,
    skip: Option<String>,
    source: String,
}

enum Outcome {
    Passed,
    Skipped { name: String, reason: String },
    Failed { name: String, reason: String },
}

#[test]
fn oclgrind_cases() {
    let cases: Vec<Case> = TABLES.iter().flat_map(|table| load(table)).collect();
    assert!(!cases.is_empty(), "no cases loaded");

    std::fs::create_dir_all(ARTIFACTS).expect("creating the artifacts directory");

    let driver = build_driver(VODD).unwrap_or_else(|error| panic!("{error}"));
    let host = build_host(&driver).unwrap_or_else(|error| panic!("{error}"));
    let filter = std::env::var("VODD_OCLGRIND_CASE").ok();

    let outcomes: Vec<Outcome> = cases
        .iter()
        .filter(|case| filter.as_ref().is_none_or(|wanted| &case.name == wanted))
        .map(|case| match &case.skip {
            Some(reason) => Outcome::Skipped { name: case.name.clone(), reason: reason.clone() },
            None => match run_case(case, &host, &driver) {
                Ok(()) => Outcome::Passed,
                Err(reason) => {
                    Outcome::Failed { name: format!("{} [{}]", case.name, case.origin), reason }
                }
            },
        })
        .collect();

    let failures: Vec<(&str, &str)> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            Outcome::Failed { name, reason } => Some((name.as_str(), reason.as_str())),
            _ => None,
        })
        .collect();

    let skipped: Vec<(&str, &str)> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            Outcome::Skipped { name, reason } => Some((name.as_str(), reason.as_str())),
            _ => None,
        })
        .collect();

    println!(
        "oclgrind: {} passed, {} failed, {} skipped, {} total",
        outcomes.len() - failures.len() - skipped.len(),
        failures.len(),
        skipped.len(),
        outcomes.len()
    );

    for (name, reason) in &skipped {
        println!("  SKIP {name}: {reason}");
    }

    for (name, reason) in &failures {
        println!("  FAIL {name}: {reason}");
    }

    assert!(failures.is_empty(), "{} cases failed", failures.len());
}

fn run_case(case: &Case, host: &Path, driver: &str) -> Result<(), String> {
    let directory = Path::new(ARTIFACTS).join(&case.name);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("creating {}: {error}", directory.display()))?;

    let source = directory.join("kernel.cl");
    let log = directory.join("diagnostics.log");

    std::fs::write(&source, &case.source)
        .map_err(|error| format!("writing {}: {error}", source.display()))?;
    let _ = std::fs::remove_file(&log);

    let output = directory.join("host.log");
    let sink = std::fs::File::create(&output)
        .map_err(|error| format!("creating {}: {error}", output.display()))?;

    let expected: Vec<String> = case
        .local_memory
        .map(|size| vec!["--expect-local-memory".to_string(), size.to_string()])
        .unwrap_or_default();

    let mut command = Command::new(host);
    command
        .args(&expected)
        .arg(&source)
        .arg(&case.entry)
        .arg(&case.global_size)
        .arg(&case.local_size)
        .args(&case.arguments)
        .env("VODD_CHECK", "all")
        .env("VODD_LOG", &log)
        .envs(runtime_environment(VODD, driver))
        .stdout(std::process::Stdio::from(sink.try_clone().map_err(
            |error| format!("duplicating the log handle: {error}"),
        )?))
        .stderr(std::process::Stdio::from(sink));

    run_with_timeout(&mut command, TIMEOUT)?;

    let host_output = std::fs::read_to_string(&output).unwrap_or_default();
    if host_output.contains("host: ") {
        return Err(host_output
            .lines()
            .next()
            .unwrap_or("host failed")
            .to_string());
    }

    compare(&counts(&log), &case.expected)
}

fn counts(log: &Path) -> Counts {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut counts = Counts::new();

    for line in text.lines().filter_map(kind_of) {
        *counts.entry(line.to_string()).or_default() += 1;
    }

    counts
}

fn kind_of(line: &str) -> Option<&'static str> {
    const KINDS: &[(&str, &str)] = &[
        ("dereference of a null pointer", "invalid_access"),
        (
            "access to an address that is not a live allocation",
            "invalid_access",
        ),
        (
            "access runs past the end of its allocation",
            "invalid_access",
        ),
        ("read from a write-only buffer", "access_flags"),
        ("write to a read-only buffer", "access_flags"),
        (
            "access to a buffer region the host currently holds mapped",
            "mapped_region",
        ),
        ("exceeds array size", "array_index"),
        ("address is not aligned to", "misaligned"),
        ("atomic on an unsupported width", "atomic_width"),
        ("work-group divergence at a barrier", "barrier_divergence"),
        ("work items reached the barrier", "barrier_participation"),
        ("data race between work item", "data_race"),
    ];

    KINDS
        .iter()
        .find(|(text, _)| line.contains(text))
        .map(|(_, kind)| *kind)
}

fn compare(produced: &Counts, expected: &Counts) -> Result<(), String> {
    match produced == expected {
        true => Ok(()),
        false => Err(format!("expected {expected:?} but reported {produced:?}")),
    }
}

fn build_host(driver: &str) -> Result<PathBuf, String> {
    let binary = Path::new(ARTIFACTS).join("host");
    let source = Path::new(HOST);

    if is_current(&binary, source) {
        return Ok(binary);
    }

    let output = Command::new("cc")
        .args(["-w", "-I", HEADERS, "-I", REDIRECT])
        .arg(source)
        .args(link_arguments(VODD, driver)?)
        .arg("-o")
        .arg(&binary)
        .output()
        .map_err(|error| format!("cc must be installed and on PATH: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "compiling {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(binary)
}

fn load(path: &str) -> Vec<Case> {
    let root = documents(path);

    root["cases"]
        .as_vec()
        .unwrap_or_else(|| panic!("{path} has no top level cases list"))
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = entry["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{path}: case {index} has no name"))
                .to_string();
            let context = format!("{path}: case {name}");

            Case {
                entry: entry["entry"].as_str().unwrap_or(&name).to_string(),
                origin: entry["origin"].as_str().unwrap_or("vodd").to_string(),
                global_size: sizes(&entry["global_size"], &context),
                local_size: sizes(&entry["local_size"], &context),
                arguments: arguments(&entry["arguments"], &context),
                local_memory: entry["local_memory"].as_i64(),
                expected: expected(&entry["expected"], &context),
                skip: entry["skip"].as_str().map(|reason| reason.to_string()),
                source: entry["source"].as_str().unwrap_or_default().to_string(),
                name,
            }
        })
        .collect()
}

fn sizes(entry: &yaml_rust2::Yaml, context: &str) -> String {
    let dimensions = entry
        .as_vec()
        .unwrap_or_else(|| panic!("{context}: sizes must be a list of three integers"));

    dimensions
        .iter()
        .map(|value| {
            value
                .as_i64()
                .unwrap_or_else(|| panic!("{context}: sizes must be integers"))
                .to_string()
        })
        .collect::<Vec<String>>()
        .join(",")
}

fn arguments(entry: &yaml_rust2::Yaml, context: &str) -> Vec<String> {
    let Some(declared) = entry.as_vec() else {
        return Vec::new();
    };

    declared
        .iter()
        .map(|argument| argument_spec(argument, context))
        .collect()
}

fn argument_spec(argument: &yaml_rust2::Yaml, context: &str) -> String {
    if argument["nullptr"].as_bool().unwrap_or(false) {
        return "null".to_string();
    }

    if let Some(size) = argument["local"].as_i64() {
        return format!("local:{size}");
    }

    if !argument["value"].is_badvalue() {
        return format!("value:{}", hex(&bytes(argument, "value", 0, context)));
    }

    let size = argument["buffer"].as_i64().unwrap_or_else(|| {
        panic!("{context}: an argument needs one of buffer, local, value, nullptr")
    }) as usize;

    let mut spec = format!("buffer:{size}");

    if let Some(access) = argument["access"].as_str() {
        spec.push(':');
        spec.push_str(access);
    }

    if let Some(mapped) = argument["mapped"].as_str() {
        spec.push_str(&format!(":map={mapped}"));
    }

    let contents = bytes(argument, "data", size, context);
    if contents.iter().any(|byte| *byte != 0) {
        spec.push_str(&format!(":data={}", hex(&contents)));
    }

    spec
}

fn bytes(argument: &yaml_rust2::Yaml, literal: &str, size: usize, context: &str) -> Vec<u8> {
    let elem = argument["elem"].as_str().unwrap_or("int");
    let width = match elem {
        "char" => 1,
        "int" | "uint" | "float" => 4,
        other => panic!("{context}: unsupported elem {other}"),
    };

    let mut contents = vec![0u8; size];

    if let Some(values) = argument[literal].as_vec() {
        let flat: Vec<i64> = values
            .iter()
            .map(|value| {
                value
                    .as_i64()
                    .unwrap_or_else(|| panic!("{context}: {literal} must be integers"))
            })
            .collect();

        contents.resize(size.max(flat.len() * width), 0);
        write_elements(&mut contents, &flat, width, elem);

        return contents;
    }

    if let Some(fill) = argument["fill"].as_i64() {
        let repeated = vec![fill; size / width];
        write_elements(&mut contents, &repeated, width, elem);

        return contents;
    }

    if let Some(range) = argument["range"].as_vec() {
        let bounds: Vec<i64> = range.iter().filter_map(yaml_rust2::Yaml::as_i64).collect();
        let [start, step, end] = bounds[..] else {
            panic!("{context}: range must be [start, step, end]");
        };

        let sequence: Vec<i64> = (0i64..)
            .map_while(|index| {
                let value = start + index * step;
                (value <= end).then_some(value)
            })
            .collect();

        write_elements(&mut contents, &sequence, width, elem);
    }

    contents
}

fn write_elements(contents: &mut [u8], values: &[i64], width: usize, elem: &str) {
    for (index, value) in values.iter().enumerate() {
        let start = index * width;
        let end = start + width;

        if end > contents.len() {
            continue;
        }

        match elem {
            "float" => contents[start..end].copy_from_slice(&(*value as f32).to_le_bytes()),
            _ => contents[start..end].copy_from_slice(&value.to_le_bytes()[..width]),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn expected(entry: &yaml_rust2::Yaml, context: &str) -> Counts {
    let Some(declared) = entry.as_hash() else {
        return Counts::new();
    };

    declared
        .iter()
        .map(|(kind, count)| {
            let kind = kind
                .as_str()
                .unwrap_or_else(|| panic!("{context}: expected keys must be strings"))
                .to_string();
            let count = count
                .as_i64()
                .unwrap_or_else(|| panic!("{context}: expected counts must be integers"))
                as usize;

            (kind, count)
        })
        .collect()
}
