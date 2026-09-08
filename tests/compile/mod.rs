#![allow(dead_code)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

pub const VODD: &str = "vodd";
pub const HEADERS: &str = "tests/OpenCL-Headers";
pub const SPIRV_HEADERS: &str = "tests/SPIRV-Headers/include";
pub const REDIRECT: &str = "tests/opencl/include";

pub fn run_with_timeout(command: &mut Command, seconds: u64) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawning the suite: {error}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);

    loop {
        match child
            .try_wait()
            .map_err(|error| format!("waiting for the suite: {error}"))?
        {
            Some(status) if status.code().is_some() => return Ok(()),
            Some(_) => return Err("terminated by signal".to_string()),
            None => {}
        }

        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();

            return Err(format!("timed out after {seconds}s"));
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

pub fn build_driver(target: &str) -> Result<String, String> {
    let driver = "target/release".to_string();

    if target != VODD {
        return Ok(driver);
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(cargo);

    command.args(["build", "--release"]);

    println!("driver [{target}]: building {driver}, which the suites link against");

    let output = command
        .output()
        .map_err(|error| format!("building the vodd driver: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "building the vodd driver: {}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .filter(|line| line.starts_with("error"))
                .take(4)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    Ok(driver)
}

pub fn link_arguments(target: &str, driver: &str) -> Result<Vec<String>, String> {
    if target != VODD {
        let loader = std::env::var("VODD_OPENCL_LOADER")
            .unwrap_or_else(|_| "/opt/homebrew/opt/opencl-icd-loader".to_string());

        return Ok(vec![format!("-L{loader}/lib"), "-lOpenCL".to_string()]);
    }

    let library = Path::new(driver).join("libvodd.dylib");
    if !library.exists() {
        return Err(format!(
            "{} is missing; run: cargo build",
            library.display()
        ));
    }

    Ok(vec![
        format!("-L{driver}"),
        "-lvodd".to_string(),
        "-Wl,-undefined,dynamic_lookup".to_string(),
    ])
}

pub fn runtime_environment(target: &str, driver: &str) -> Vec<(String, String)> {
    let library_path = (target == VODD).then(|| {
        (
            "DYLD_LIBRARY_PATH".to_string(),
            std::fs::canonicalize(driver)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| driver.to_string()),
        )
    });

    library_path.into_iter().chain(macos_sdk()).collect()
}

pub fn macos_sdk() -> Vec<(String, String)> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }

    let Ok(output) = Command::new("xcrun").args(["--show-sdk-path"]).output() else {
        return Vec::new();
    };

    let sdk = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sdk.is_empty() {
        return Vec::new();
    }

    vec![
        ("SDKROOT".to_string(), sdk.clone()),
        ("LIBRARY_PATH".to_string(), format!("{sdk}/usr/lib")),
    ]
}

pub fn is_current(object: &Path, source: &Path) -> bool {
    match (modified(object), modified(source)) {
        (Some(built), Some(edited)) => built > edited,
        _ => false,
    }
}

pub fn newest(paths: &[PathBuf]) -> Option<std::time::SystemTime> {
    paths.iter().filter_map(|path| modified(path)).max()
}

pub fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

pub fn string_field<'y>(entry: &'y yaml_rust2::Yaml, name: &str, context: &str) -> &'y str {
    entry[name]
        .as_str()
        .unwrap_or_else(|| panic!("{context}: {name} is missing or is not a string"))
}

pub fn integer_field(entry: &yaml_rust2::Yaml, name: &str, context: &str) -> usize {
    entry[name]
        .as_i64()
        .unwrap_or_else(|| panic!("{context}: {name} is missing or is not an integer")) as usize
}

pub fn documents(path: &str) -> yaml_rust2::Yaml {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));

    yaml_rust2::YamlLoader::load_from_str(&text)
        .unwrap_or_else(|error| panic!("{path} is not valid YAML: {error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{path} is empty"))
}

pub fn compile_c(
    source: &Path,
    object: &Path,
    compiler: &str,
    arguments: &[String],
) -> Result<(), String> {
    if is_current(object, source) {
        return Ok(());
    }

    let output = Command::new(compiler)
        .args(arguments)
        .arg("-c")
        .arg(source)
        .arg("-o")
        .arg(object)
        .output()
        .map_err(|error| format!("{compiler} must be installed and on PATH: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "compiling {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(())
}
