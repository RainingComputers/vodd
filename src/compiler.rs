use crate::logger;
use crate::logger::Level;

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

const LINKERS: &[&str] = &[
    "/opt/homebrew/bin/spirv-link",
    "/home/linuxbrew/.linuxbrew/bin/spirv-link",
    "/usr/local/bin/spirv-link",
    "spirv-link",
];
const TRANSLATORS: &[&str] = &[
    "/opt/homebrew/opt/spirv-llvm-translator/bin/llvm-spirv",
    "/home/linuxbrew/.linuxbrew/opt/spirv-llvm-translator/bin/llvm-spirv",
    "/usr/local/opt/spirv-llvm-translator/bin/llvm-spirv",
    "llvm-spirv",
    "llvm-spirv-21",
    "llvm-spirv-20",
];
const COMPILERS: &[&str] = &[
    "/opt/homebrew/opt/llvm/bin/clang",
    "/home/linuxbrew/.linuxbrew/opt/llvm/bin/clang",
    "/opt/homebrew/opt/llvm@21/bin/clang",
    "/usr/local/opt/llvm/bin/clang",
    "clang",
    "clang-21",
    "clang-20",
    "clang-19",
    "clang-18",
];
const SPIRV_VERSION: &str = "1.1";
const MINIMUM_CLANG: u32 = 23;
const MINIMUM_TRANSLATOR: u32 = 23;
const MINIMUM_LINKER: (u32, u32) = (2026, 3);
const UNUSED: &str = "-Wno-unused-command-line-argument";
const SUPPLIED: &[&str] = &[
    "atomic_inc",
    "atomic_dec",
    "atomic_min",
    "atomic_max",
    "atomic_xchg",
];
const PRELUDE: &str = r#"#define VODD_OVERLOAD __attribute__((overloadable))
#define VODD_RMW(name, type, space, expr) \
    VODD_OVERLOAD type name(volatile space type *p, type v) \
    { \
        type old = *p; \
        for (;;) { \
            type want = (expr); \
            type prev = atomic_cmpxchg(p, old, want); \
            if (prev == old) return old; \
            old = prev; \
        } \
    }
#define VODD_ATOMICS(space) \
    VODD_OVERLOAD int atomic_inc(volatile space int *p) { return atomic_add(p, 1); } \
    VODD_OVERLOAD int atomic_dec(volatile space int *p) { return atomic_sub(p, 1); } \
    VODD_OVERLOAD uint atomic_inc(volatile space uint *p) { return atomic_add(p, 1u); } \
    VODD_OVERLOAD uint atomic_dec(volatile space uint *p) { return atomic_sub(p, 1u); } \
    VODD_RMW(atomic_min, int, space, old < v ? old : v) \
    VODD_RMW(atomic_max, int, space, old > v ? old : v) \
    VODD_RMW(atomic_xchg, int, space, v) \
    VODD_RMW(atomic_min, uint, space, old < v ? old : v) \
    VODD_RMW(atomic_max, uint, space, old > v ? old : v) \
    VODD_RMW(atomic_xchg, uint, space, v)
VODD_ATOMICS(global)
VODD_ATOMICS(local)
#undef VODD_ATOMICS
#undef VODD_RMW
#undef VODD_OVERLOAD
"#;

static CLANG: OnceLock<Option<PathBuf>> = OnceLock::new();
static LINKER: OnceLock<Option<PathBuf>> = OnceLock::new();
static TRANSLATOR: OnceLock<Option<PathBuf>> = OnceLock::new();
static NEXT_SCRATCH: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    Missing,
    Spawn,
    Staging,
}

pub type Result<T> = core::result::Result<T, Error>;

pub struct Output {
    pub binary: Option<Vec<u8>>,
    pub log: String,
}

pub fn link(objects: &[Vec<u8>], library: bool) -> Result<Output> {
    let linker = linker().ok_or(Error::Missing)?;
    let scratch = Scratch::new()?;

    let inputs = objects
        .iter()
        .enumerate()
        .map(|(index, object)| {
            let path = scratch.path.join(format!("input{index}.spv"));

            std::fs::write(&path, object)
                .map(|()| path)
                .map_err(|error| staging("a linker input", error))
        })
        .collect::<Result<Vec<PathBuf>>>()?;

    let produced = scratch.path.join("linked.spv");
    let mut command = Command::new(linker);

    command.args(&inputs).arg("-o").arg(&produced);
    if library {
        command.arg("--create-library");
    }

    let output = command.output().map_err(|error| spawning(linker, error))?;

    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    let binary = output.status.success().then(|| read(&produced)).flatten();

    Ok(Output { binary, log })
}

pub fn compile(
    source: &str,
    headers: &[(String, String)],
    options: &str,
    enable_opt: Option<bool>,
) -> Result<Output> {
    let clang = clang().ok_or(Error::Missing)?;
    let translator = translator().ok_or(Error::Missing)?;
    let scratch = Scratch::new()?;

    headers.iter().try_for_each(|(name, text)| {
        let path = scratch.path.join(name);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| staging("a header", error))?;
        }

        std::fs::write(path, text).map_err(|error| staging("a header", error))
    })?;

    let input = scratch.path.join("source.cl");

    std::fs::write(&input, source).map_err(|error| staging("the source", error))?;

    let prelude = match SUPPLIED.iter().any(|name| source.contains(name)) {
        true => {
            let path = scratch.path.join("prelude.h");

            std::fs::write(&path, PRELUDE).map_err(|error| staging("the prelude", error))?;

            Some(path)
        }
        false => None,
    };

    let produced = scratch.path.join("source.spv");
    let emitted = scratch.path.join("source.bc");

    let mut command = Command::new(clang);

    command
        .args(arguments(options, enable_opt))
        .arg(format!("-I{}", scratch.path.display()));

    if let Some(prelude) = &prelude {
        command.arg("-include").arg(prelude);
    }

    let output = command
        .arg("-o")
        .arg(&emitted)
        .arg(&input)
        .output()
        .map_err(|error| spawning(clang, error))?;

    let mut log = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        return Ok(Output { binary: None, log });
    }

    let translated = Command::new(translator)
        .arg(&emitted)
        .arg("-o")
        .arg(&produced)
        .arg("--spirv-debug-info-version=nonsemantic-shader-200")
        .arg(format!("--spirv-max-version={SPIRV_VERSION}"))
        .output()
        .map_err(|error| spawning(translator, error))?;

    log.push_str(&String::from_utf8_lossy(&translated.stderr));

    if !translated.status.success() {
        return Ok(Output { binary: None, log });
    }

    Ok(Output { binary: read(&produced), log })
}

fn arguments(options: &str, enable_opt: Option<bool>) -> Vec<String> {
    let disabled = options
        .split_whitespace()
        .any(|option| option == "-cl-opt-disable");

    let optimisation = match (enable_opt, disabled) {
        (Some(false), _) | (_, true) => "-O0",
        _ => "-O2",
    };

    ["-x", "cl", "-cl-std=CL1.2"]
        .iter()
        .chain(["--target=spir64", "-g", "-gembed-source", "-emit-llvm"].iter())
        .chain([optimisation].iter())
        .chain(["-Xclang", "-finclude-default-header", "-c", UNUSED].iter())
        .map(|argument| argument.to_string())
        .chain(options.split_whitespace().map(|option| option.to_string()))
        .collect()
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self> {
        let unique = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vodd-{}-{unique}", std::process::id()));

        std::fs::create_dir_all(&path).map_err(|error| staging("the scratch directory", error))?;

        Ok(Self { path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            logger::log(
                Level::Warn,
                &format!(
                    "the scratch directory {} could not be removed, {error}",
                    self.path.display()
                ),
            );
        }
    }
}

pub fn available() -> bool {
    clang().is_some() && translator().is_some()
}

pub fn linkable() -> bool {
    linker().is_some()
}

pub fn requirements() -> String {
    let clang = clang().and_then(major_version);
    let translator = translator().and_then(major_version);
    let linker = linker().and_then(linker_version);

    format!(
        "clang >= {MINIMUM_CLANG} (found {}), llvm-spirv >= {MINIMUM_TRANSLATOR} (found {}) \
         and spirv-link >= v{}.{} (found {}); set VODD_CLANG, VODD_LLVM_SPIRV and \
         VODD_SPIRV_LINK to point at them",
        describe(clang),
        describe(translator),
        MINIMUM_LINKER.0,
        MINIMUM_LINKER.1,
        describe_linker(linker)
    )
}

fn linker() -> Option<&'static PathBuf> {
    LINKER
        .get_or_init(|| {
            let found = std::env::var("VODD_SPIRV_LINK")
                .ok()
                .map(PathBuf::from)
                .into_iter()
                .chain(LINKERS.iter().map(PathBuf::from))
                .find(|candidate| {
                    linker_version(candidate).is_some_and(|found| found >= MINIMUM_LINKER)
                });

            announce("spirv-link", found, Level::Warn)
        })
        .as_ref()
}

fn translator() -> Option<&'static PathBuf> {
    TRANSLATOR
        .get_or_init(|| {
            let found = std::env::var("VODD_LLVM_SPIRV")
                .ok()
                .map(PathBuf::from)
                .into_iter()
                .chain(TRANSLATORS.iter().map(PathBuf::from))
                .find(|candidate| {
                    major_version(candidate).is_some_and(|major| major >= MINIMUM_TRANSLATOR)
                });

            announce("llvm-spirv", found, Level::Error)
        })
        .as_ref()
}

fn clang() -> Option<&'static PathBuf> {
    CLANG
        .get_or_init(|| {
            let found = std::env::var("VODD_CLANG")
                .ok()
                .map(PathBuf::from)
                .into_iter()
                .chain(COMPILERS.iter().map(PathBuf::from))
                .find(|candidate| {
                    major_version(candidate).is_some_and(|major| major >= MINIMUM_CLANG)
                        && emits_spirv(candidate)
                });

            announce("clang", found, Level::Error)
        })
        .as_ref()
}

fn emits_spirv(clang: &PathBuf) -> bool {
    Command::new(clang)
        .args(arguments("", None))
        .args(["-o", "/dev/null", "/dev/null"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn major_version(tool: &PathBuf) -> Option<u32> {
    let output = Command::new(tool).arg("--version").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let after = text.split("version").nth(1)?;
    let digits: String = after
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();

    digits.parse().ok()
}

fn announce(tool: &str, found: Option<PathBuf>, absent: Level) -> Option<PathBuf> {
    match &found {
        Some(path) => logger::log(Level::Info, &format!("using {tool} at {}", path.display())),
        None => logger::log(absent, &format!("no usable {tool} was found")),
    }

    found
}

fn staging(what: &str, error: std::io::Error) -> Error {
    logger::log(
        Level::Error,
        &format!("{what} could not be written to the scratch directory, {error}"),
    );

    Error::Staging
}

fn spawning(tool: &Path, error: std::io::Error) -> Error {
    logger::log(
        Level::Error,
        &format!("{} could not be run, {error}", tool.display()),
    );

    Error::Spawn
}

fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path)
        .inspect_err(|error| {
            logger::log(
                Level::Error,
                &format!(
                    "{} was reported as written but could not be read, {error}",
                    path.display()
                ),
            );
        })
        .ok()
}

fn describe(major: Option<u32>) -> String {
    major.map_or_else(|| "none".to_string(), |found| found.to_string())
}

fn describe_linker(release: Option<(u32, u32)>) -> String {
    release.map_or_else(
        || "none".to_string(),
        |(year, revision)| format!("v{year}.{revision}"),
    )
}

fn linker_version(tool: &PathBuf) -> Option<(u32, u32)> {
    let output = Command::new(tool).arg("--version").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let tagged = text
        .split_whitespace()
        .find(|word| word.starts_with('v') && word[1..].split('.').all(|part| !part.is_empty()))?;

    let mut parts = tagged[1..].split('.');
    let year = parts.next()?.parse().ok()?;
    let revision = parts.next().unwrap_or("0").parse().ok()?;

    Some((year, revision))
}
