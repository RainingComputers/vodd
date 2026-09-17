#![allow(dead_code)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use super::VODD;

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
            .ok()
            .or_else(default_loader);

        let mut arguments = Vec::new();
        if let Some(loader) = loader {
            arguments.push(format!("-L{loader}/lib"));
        }
        arguments.push("-lOpenCL".to_string());

        return Ok(arguments);
    }

    let library = Path::new(driver).join(library_name());
    if !library.exists() {
        return Err(format!(
            "{} is missing; run: cargo build --release",
            library.display()
        ));
    }

    let mut arguments = vec![format!("-L{driver}"), "-lvodd".to_string()];

    match cfg!(target_os = "macos") {
        true => arguments.push("-Wl,-undefined,dynamic_lookup".to_string()),
        false => arguments.push(format!("-Wl,-rpath,{}", absolute(driver))),
    }

    Ok(arguments)
}

pub fn library_name() -> &'static str {
    match cfg!(target_os = "macos") {
        true => "libvodd.dylib",
        false => "libvodd.so",
    }
}

fn default_loader() -> Option<String> {
    let homebrew = "/opt/homebrew/opt/opencl-icd-loader";
    match cfg!(target_os = "macos") {
        true => Some(homebrew.to_string()),
        false => None,
    }
}

pub fn runtime_environment(target: &str, driver: &str) -> Vec<(String, String)> {
    let library_path =
        (target == VODD).then(|| (library_path_variable().to_string(), absolute(driver)));

    library_path.into_iter().chain(macos_sdk()).collect()
}

pub fn library_path_variable() -> &'static str {
    match cfg!(target_os = "macos") {
        true => "DYLD_LIBRARY_PATH",
        false => "LD_LIBRARY_PATH",
    }
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

fn absolute(driver: &str) -> String {
    std::fs::canonicalize(driver)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| driver.to_string())
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
