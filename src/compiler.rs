use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

const LINKERS: &[&str] = &[
    "/opt/homebrew/bin/spirv-link",
    "/usr/local/bin/spirv-link",
    "spirv-link",
];
const COMPILERS: &[&str] = &[
    "/opt/homebrew/opt/llvm/bin/clang",
    "/opt/homebrew/opt/llvm@21/bin/clang",
    "/usr/local/opt/llvm/bin/clang",
    "clang",
];
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
#line 1
"#;

static CLANG: OnceLock<Option<PathBuf>> = OnceLock::new();
static LINKER: OnceLock<Option<PathBuf>> = OnceLock::new();
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
                .map_err(|_| Error::Staging)
        })
        .collect::<Result<Vec<PathBuf>>>()?;

    let produced = scratch.path.join("linked.spv");
    let mut command = Command::new(linker);

    command.args(&inputs).arg("-o").arg(&produced);
    if library {
        command.arg("--create-library");
    }

    let output = command.output().map_err(|_| Error::Spawn)?;

    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    let binary = output
        .status
        .success()
        .then(|| std::fs::read(&produced).ok())
        .flatten();

    Ok(Output { binary, log })
}

pub fn compile(source: &str, headers: &[(String, String)], options: &str) -> Result<Output> {
    let clang = clang().ok_or(Error::Missing)?;
    let scratch = Scratch::new()?;

    headers.iter().try_for_each(|(name, text)| {
        let path = scratch.path.join(name);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| Error::Staging)?;
        }

        std::fs::write(path, text).map_err(|_| Error::Staging)
    })?;

    let input = scratch.path.join("source.cl");
    let prepared = if SUPPLIED.iter().any(|name| source.contains(name)) {
        format!("{PRELUDE}{source}")
    } else {
        source.to_string()
    };

    std::fs::write(&input, prepared).map_err(|_| Error::Staging)?;

    let produced = scratch.path.join("source.spv");

    let output = Command::new(clang)
        .args(arguments(options))
        .arg(format!("-I{}", scratch.path.display()))
        .arg("-o")
        .arg(&produced)
        .arg(&input)
        .output()
        .map_err(|_| Error::Spawn)?;

    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    let binary = output
        .status
        .success()
        .then(|| std::fs::read(&produced).ok())
        .flatten();

    Ok(Output { binary, log })
}

fn arguments(options: &str) -> Vec<String> {
    [
        "-x",
        "cl",
        "-cl-std=CL1.2",
        "--target=spirv64v1.0",
        "-Xclang",
        "-finclude-default-header",
        "-c",
    ]
    .iter()
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

        std::fs::create_dir_all(&path).map_err(|_| Error::Staging)?;

        Ok(Self { path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

pub fn available() -> bool {
    clang().is_some()
}

pub fn linkable() -> bool {
    linker().is_some()
}

fn linker() -> Option<&'static PathBuf> {
    LINKER
        .get_or_init(|| {
            std::env::var("VODD_SPIRV_LINK")
                .ok()
                .map(PathBuf::from)
                .into_iter()
                .chain(LINKERS.iter().map(PathBuf::from))
                .find(|candidate| {
                    Command::new(candidate)
                        .arg("--version")
                        .output()
                        .is_ok_and(|output| output.status.success())
                })
        })
        .as_ref()
}

fn clang() -> Option<&'static PathBuf> {
    CLANG
        .get_or_init(|| {
            std::env::var("VODD_CLANG")
                .ok()
                .map(PathBuf::from)
                .into_iter()
                .chain(COMPILERS.iter().map(PathBuf::from))
                .find(targets_spirv)
        })
        .as_ref()
}

fn targets_spirv(clang: &PathBuf) -> bool {
    Command::new(clang)
        .arg("-print-targets")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("spirv64"))
}
