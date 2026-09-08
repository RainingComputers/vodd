use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

mod compile;

use compile::HEADERS;
use compile::REDIRECT;
use compile::SPIRV_HEADERS;
use compile::VODD;
use compile::build_driver;
use compile::is_current;
use compile::link_arguments;
use compile::modified;
use compile::newest;
use compile::run_with_timeout;
use compile::runtime_environment;

const TIMEOUT: u64 = 300;
const CTS: &str = "tests/OpenCL-CTS";
const HARNESS_SOURCES: &[&str] = &[
    "harness/alloc.cpp",
    "harness/typeWrappers.cpp",
    "harness/mt19937.cpp",
    "harness/conversions.cpp",
    "harness/rounding_mode.cpp",
    "harness/crc32.cpp",
    "harness/errorHelpers.cpp",
    "harness/featureHelpers.cpp",
    "harness/genericThread.cpp",
    "harness/imageHelpers.cpp",
    "harness/kernelHelpers.cpp",
    "harness/deviceInfo.cpp",
    "harness/os_helpers.cpp",
    "harness/parseParameters.cpp",
    "harness/propertyHelpers.cpp",
    "harness/testHarness.cpp",
    "harness/ThreadPool.cpp",
    "miniz/miniz.c",
];

type StringMap = BTreeMap<String, String>;

struct OpenclCase {
    suite: String,
    expected: StringMap,
    excluded: StringMap,
}

enum OpenclCaseResult {
    Passed,
    Failed { suite: String, reason: String },
}

impl OpenclCaseResult {
    fn failure(&self) -> Option<(&str, &str)> {
        match self {
            OpenclCaseResult::Passed => None,
            OpenclCaseResult::Failed { suite, reason } => Some((suite, reason)),
        }
    }
}

#[test]
fn opencl_cases() {
    let target = std::env::var("VODD_OPENCL_TARGET").unwrap_or_else(|_| VODD.to_string());
    let opencl_cases = load_opencl_cases(&target);
    assert!(!opencl_cases.is_empty(), "no OpenCL cases loaded");

    let driver = build_driver(&target).unwrap_or_else(|error| panic!("{error}"));
    let artifacts = "target/opencl";

    let harness = build_harness(artifacts).unwrap_or_else(|error| panic!("{error}"));
    let filter = std::env::var("VODD_OPENCL_CASE").ok();

    let results: Vec<OpenclCaseResult> = opencl_cases
        .iter()
        .filter(|opencl_case| {
            filter
                .as_ref()
                .is_none_or(|wanted| &opencl_case.suite == wanted)
        })
        .map(|opencl_case| {
            match run_opencl_case(opencl_case, &harness, &target, &driver, artifacts)
                .and_then(|produced| compare(opencl_case, &produced))
            {
                Ok(()) => OpenclCaseResult::Passed,
                Err(reason) => {
                    OpenclCaseResult::Failed { suite: opencl_case.suite.clone(), reason }
                }
            }
        })
        .collect();

    let failures: Vec<(&str, &str)> = results
        .iter()
        .filter_map(OpenclCaseResult::failure)
        .collect();

    println!(
        "conformance [{target}]: {} passed, {} failed, {} total",
        results.len() - failures.len(),
        failures.len(),
        results.len()
    );

    for (suite, reason) in &failures {
        println!("  FAIL {suite}: {reason}");
    }

    assert!(
        failures.is_empty(),
        "{} OpenCL suites failed",
        failures.len()
    );
}

fn run_opencl_case(
    opencl_case: &OpenclCase,
    harness: &[PathBuf],
    target: &str,
    driver: &str,
    artifacts: &str,
) -> Result<StringMap, String> {
    let binary = build_suite(&opencl_case.suite, harness, target, driver, artifacts)?;

    let undeclared: Vec<String> = list_tests(&binary)?
        .into_iter()
        .filter(|name| {
            !opencl_case.expected.contains_key(name) && !opencl_case.excluded.contains_key(name)
        })
        .take(4)
        .collect();
    if !undeclared.is_empty() {
        return Err(format!(
            "not declared in expected or excluded: {}",
            undeclared.join(", ")
        ));
    }

    let directory = Path::new(artifacts).join(target).join(&opencl_case.suite);
    let produced = directory.join("results.json");
    let log = directory.join("run.log");
    let sink = std::fs::File::create(&log)
        .map_err(|error| format!("creating {}: {error}", log.display()))?;

    let _ = std::fs::remove_file(&produced);

    let mut command = Command::new(&binary);
    command
        .env("CL_CONFORMANCE_RESULTS_FILENAME", &produced)
        .envs(runtime_environment(target, driver))
        .args(opencl_case.expected.keys())
        .stdout(Stdio::from(sink.try_clone().map_err(|error| {
            format!("duplicating the log handle: {error}")
        })?))
        .stderr(Stdio::from(sink));

    run_with_timeout(&mut command, TIMEOUT)
        .map_err(|error| format!("{}: {error}", binary.display()))?;

    let text = std::fs::read_to_string(&produced)
        .map_err(|error| format!("reading {}: {error}", produced.display()))?;

    parse_results(&text)
}

fn list_tests(binary: &Path) -> Result<Vec<String>, String> {
    let output = Command::new(binary)
        .arg("--list")
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("listing {}: {error}", binary.display()))?;

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with('\t'))
        .map(|line| line.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect())
}

fn build_suite(
    suite: &str,
    harness: &[PathBuf],
    target: &str,
    driver: &str,
    artifacts: &str,
) -> Result<PathBuf, String> {
    let source_directory = Path::new(CTS).join("test_conformance").join(suite);
    let directory = Path::new(artifacts).join(target).join(suite);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("creating {}: {error}", directory.display()))?;

    generate_sources(suite, &directory)?;

    let binary = directory.join("suite");
    let sources = source_files(&source_directory)?;

    if modified(&binary).is_some_and(|built| Some(built) > newest(&sources)) {
        return Ok(binary);
    }

    let output = Command::new("c++")
        .args(compile_arguments())
        .arg("-I")
        .arg(&source_directory)
        .arg("-I")
        .arg(&directory)
        .args(&sources)
        .args(harness)
        .args(link_arguments(target, driver)?)
        .arg("-o")
        .arg(&binary)
        .output()
        .map_err(|error| format!("c++: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "build: {}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .filter(|line| line.contains("error") || line.starts_with("  \""))
                .take(4)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    Ok(binary)
}

fn build_harness(artifacts: &str) -> Result<Vec<PathBuf>, String> {
    if !Path::new(CTS).join("test_common").is_dir() {
        return Err(format!(
            "{CTS} is empty; run: git submodule update --init --recursive"
        ));
    }

    let directory = Path::new(artifacts).join("harness");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("creating {}: {error}", directory.display()))?;

    HARNESS_SOURCES
        .iter()
        .map(|source| {
            let path = Path::new(CTS).join("test_common").join(source);
            let object = directory.join(format!(
                "{}.o",
                path.file_stem()
                    .expect("every HARNESS_SOURCES entry names a file")
                    .to_string_lossy()
            ));

            if is_current(&object, &path) {
                return Ok(object);
            }

            let output = Command::new("c++")
                .args(compile_arguments())
                .arg("-c")
                .arg(&path)
                .arg("-o")
                .arg(&object)
                .output()
                .map_err(|error| format!("c++ must be installed and on PATH: {error}"))?;

            if !output.status.success() {
                return Err(format!(
                    "compiling {}: {}",
                    path.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }

            Ok(object)
        })
        .collect()
}

fn generate_sources(suite: &str, directory: &Path) -> Result<(), String> {
    if suite != "api" {
        return Ok(());
    }

    let generated = directory.join("spirv_capability_deps.def");
    if generated.exists() {
        return Ok(());
    }

    let script = Path::new(CTS).join("test_conformance/api/generate_spirv_capability_deps.py");
    let grammar = Path::new(SPIRV_HEADERS).join("spirv/unified1/spirv.core.grammar.json");

    let output = Command::new("python3")
        .arg(&script)
        .arg("--grammar")
        .arg(&grammar)
        .arg("--output")
        .arg(&generated)
        .output()
        .map_err(|error| format!("python3 is required to build the api suite: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "generating {}: {}",
            generated.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(())
}

fn compile_arguments() -> Vec<String> {
    [
        "-std=c++17",
        "-w",
        "-DCL_ENABLE_BETA_EXTENSIONS",
        "-DCL_USE_DEPRECATED_OPENCL_1_0_APIS",
        "-DCL_USE_DEPRECATED_OPENCL_1_1_APIS",
        "-DCL_USE_DEPRECATED_OPENCL_1_2_APIS",
        "-I",
        HEADERS,
        "-I",
        REDIRECT,
        "-I",
        SPIRV_HEADERS,
        "-I",
        &format!("{CTS}/test_common"),
        "-I",
        &format!("{CTS}/test_common/harness"),
    ]
    .iter()
    .map(|argument| argument.to_string())
    .collect()
}

fn load_opencl_cases(target: &str) -> Vec<OpenclCase> {
    let path = "tests/opencl.yaml";
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let documents = yaml_rust2::YamlLoader::load_from_str(&text)
        .unwrap_or_else(|error| panic!("{path} is not valid YAML: {error}"));
    let root = &documents[0];

    root["opencl_cases"]
        .as_vec()
        .unwrap_or_else(|| panic!("{path} has no top level opencl_cases list"))
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let suite = entry["suite"]
                .as_str()
                .unwrap_or_else(|| panic!("{path}: OpenCL case {index} has no suite name"))
                .to_string();
            let context = format!("OpenCL suite {suite}");

            let expected = string_map(
                &entry["expected"][target],
                &format!("{context}: expected for target {target}"),
            );

            for (name, status) in &expected {
                assert!(
                    ["pass", "fail", "skip"].contains(&status.as_str()),
                    "{context}: {name} has unknown status {status}"
                );
            }

            let excluded = &entry["excluded"][target];

            let optional = |key: &str| {
                let value = &entry[key][target];

                if value.is_badvalue() {
                    StringMap::new()
                } else {
                    string_map(value, &format!("{context}: {key} for target {target}"))
                }
            };

            justify(
                &context,
                &expected,
                &optional("defects"),
                &optional("skip_reasons"),
            );

            OpenclCase {
                expected,
                excluded: if excluded.is_badvalue() {
                    StringMap::new()
                } else {
                    string_map(
                        excluded,
                        &format!("{context}: excluded for target {target}"),
                    )
                },
                suite,
            }
        })
        .collect()
}

fn justify(context: &str, expected: &StringMap, defects: &StringMap, reasons: &StringMap) {
    let undocumented = |status: &str, recorded: &StringMap| {
        expected
            .iter()
            .filter(|(name, actual)| *actual == status && !recorded.contains_key(*name))
            .map(|(name, _)| name.clone())
            .collect::<Vec<String>>()
    };

    let stale = |status: &str, recorded: &StringMap| {
        recorded
            .keys()
            .filter(|name| expected.get(*name).is_none_or(|actual| actual != status))
            .cloned()
            .collect::<Vec<String>>()
    };

    let complaints = [
        (
            "fail without a defects entry",
            undocumented("fail", defects),
        ),
        (
            "skip without a skip_reasons entry",
            undocumented("skip", reasons),
        ),
        (
            "defects entry for a test that does not fail",
            stale("fail", defects),
        ),
        (
            "skip_reasons entry for a test that does not skip",
            stale("skip", reasons),
        ),
    ];

    let reported: Vec<String> = complaints
        .iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(label, names)| format!("{label}: {}", names.join(", ")))
        .collect();

    assert!(reported.is_empty(), "{context}: {}", reported.join("; "));
}

fn string_map(entry: &yaml_rust2::Yaml, context: &str) -> StringMap {
    entry
        .as_hash()
        .unwrap_or_else(|| panic!("{context} is missing or is not a map"))
        .iter()
        .map(|(key, value)| {
            let key = key
                .as_str()
                .unwrap_or_else(|| panic!("{context}: a key is not a string"))
                .to_string();
            let value = value
                .as_str()
                .unwrap_or_else(|| panic!("{context}: the value of {key} is not a string"));

            (key, value.to_string())
        })
        .collect()
}

fn parse_results(text: &str) -> Result<StringMap, String> {
    let documents = yaml_rust2::YamlLoader::load_from_str(text)
        .map_err(|error| format!("results are not parseable: {error}"))?;

    documents[0]["results"]
        .as_hash()
        .ok_or_else(|| "results file has no results map".to_string())?
        .iter()
        .map(|(name, status)| {
            let name = name
                .as_str()
                .ok_or_else(|| "a test name is not a string".to_string())?;
            let status = status
                .as_str()
                .ok_or_else(|| "a test status is not a string".to_string())?;

            Ok((name.to_string(), status.to_string()))
        })
        .collect()
}

fn compare(opencl_case: &OpenclCase, produced: &StringMap) -> Result<(), String> {
    let mismatched: Vec<String> = opencl_case
        .expected
        .iter()
        .filter_map(|(name, expected)| match produced.get(name) {
            Some(status) if status == expected => None,
            Some(status) => Some(format!("{name}: expected {expected} but was {status}")),
            None => Some(format!("{name}: did not run")),
        })
        .chain(
            produced
                .keys()
                .filter(|name| !opencl_case.expected.contains_key(*name))
                .map(|name| format!("{name}: ran but is not declared in expected")),
        )
        .take(4)
        .collect();

    if mismatched.is_empty() {
        Ok(())
    } else {
        Err(mismatched.join("; "))
    }
}

fn source_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut sources: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|error| format!("reading {}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("reading {}: {error}", directory.display()))
        })
        .collect::<Result<Vec<PathBuf>, String>>()?
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "cpp" || extension == "c")
        })
        .collect();

    sources.sort();

    Ok(sources)
}
