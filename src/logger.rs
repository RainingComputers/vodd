use crate::address;
use crate::detectors;

use crate::bitcode;

use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

const DEFAULT_MAX_ERRORS: u32 = 1000;

static MAX_ERRORS: OnceLock<u32> = OnceLock::new();
static EMITTED: AtomicU32 = AtomicU32::new(0);

impl fmt::Display for bitcode::MemoryOrder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            bitcode::MemoryOrder::Relaxed => write!(formatter, "relaxed"),
            bitcode::MemoryOrder::Acquire => write!(formatter, "acquire"),
            bitcode::MemoryOrder::Release => write!(formatter, "release"),
            bitcode::MemoryOrder::AcquireRelease => write!(formatter, "acquire-release"),
            bitcode::MemoryOrder::SequentiallyConsistent => write!(formatter, "seq-cst"),
        }
    }
}

impl fmt::Display for bitcode::MemorySpace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            bitcode::MemorySpace::None => write!(formatter, "no memory"),
            bitcode::MemorySpace::Workgroup => write!(formatter, "workgroup"),
            bitcode::MemorySpace::CrossWorkgroup => write!(formatter, "cross-workgroup"),
            bitcode::MemorySpace::Both => write!(formatter, "workgroup+cross-workgroup"),
        }
    }
}

impl fmt::Display for bitcode::MemorySemantics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.order, self.space)
    }
}

impl fmt::Display for address::Region {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            address::Region::Global => write!(formatter, "global"),
            address::Region::Local => write!(formatter, "local"),
            address::Region::Mutable => write!(formatter, "mutable"),
            address::Region::Invocation => write!(formatter, "private"),
            address::Region::Builtin => write!(formatter, "builtin"),
            address::Region::Constant => write!(formatter, "constant"),
        }
    }
}

impl fmt::Display for detectors::Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            detectors::Severity::Error => write!(formatter, "error"),
            detectors::Severity::Warning => write!(formatter, "warning"),
        }
    }
}

impl fmt::Display for detectors::Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            detectors::Kind::InvalidAccess { reason: address::Invalid::Null } => {
                write!(formatter, "dereference of a null pointer")
            }
            detectors::Kind::InvalidAccess { reason: address::Invalid::Unmapped } => {
                write!(
                    formatter,
                    "access to an address that is not a live allocation"
                )
            }
            detectors::Kind::InvalidAccess { reason: address::Invalid::Overrun } => {
                write!(formatter, "access runs past the end of its allocation")
            }
            detectors::Kind::AccessFlags { writable: true } => {
                write!(formatter, "read from a write-only buffer")
            }
            detectors::Kind::AccessFlags { writable: false } => {
                write!(formatter, "write to a read-only buffer")
            }
            detectors::Kind::MappedRegion => write!(
                formatter,
                "access to a buffer region the host currently holds mapped"
            ),
            detectors::Kind::ArrayIndex { index, count } => {
                write!(formatter, "index {index} exceeds array size {count}")
            }
            detectors::Kind::Misaligned { alignment } => {
                write!(formatter, "address is not aligned to {alignment} bytes")
            }
            detectors::Kind::AtomicWidth { width } => {
                write!(formatter, "atomic on an unsupported width of {width} bits")
            }
            detectors::Kind::BarrierDivergence { reached, expected } => write!(
                formatter,
                "work-group divergence at a barrier (scope {:#x} semantics {}, \
                 but other work items reached scope {:#x} semantics {})",
                reached.execution_scope,
                reached.semantics,
                expected.execution_scope,
                expected.semantics
            ),
            detectors::Kind::BarrierParticipation { arrived, total } => write!(
                formatter,
                "only {arrived} out of {total} work items reached the barrier"
            ),
            detectors::Kind::DataRace { first, second } => write!(
                formatter,
                "data race between work item {} and work item {}",
                triple(first.global),
                triple(second.global)
            ),
        }
    }
}

impl fmt::Display for detectors::Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.severity, self.kind)?;

        if let Some(access) = &self.access {
            let operation = if access.store { "write" } else { "read" };
            let atomic = if access.atomic { "atomic " } else { "" };

            let region: &dyn fmt::Display = match &access.region {
                Some(region) => region,
                None => &"untagged",
            };

            write!(
                formatter,
                " ({atomic}{operation} of {} bytes at {region} address {:#x})",
                access.size, access.address
            )?;
        }

        if let Some(entity) = &self.entity {
            write!(
                formatter,
                "\n  work item {}, work group {}, local id {}",
                triple(entity.global),
                triple(entity.group),
                triple(entity.local)
            )?;
        }

        Ok(())
    }
}

fn triple(value: [u64; 3]) -> String {
    format!("({}, {}, {})", value[0], value[1], value[2])
}

pub fn log(message: &str) {
    let seen = EMITTED.fetch_add(1, Ordering::Relaxed);
    let limit = max_errors();

    if seen >= limit {
        return;
    }

    write(message);

    if seen + 1 == limit {
        write(&format!(
            "vodd: {limit} diagnostics reported, suppressing further output"
        ));
    }
}

fn write(message: &str) {
    use std::io::Write;

    match std::env::var("VODD_LOG").ok() {
        Some(path) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{message}");
            }
        }
        None => {
            let _ = writeln!(std::io::stderr(), "{message}");
        }
    }
}

fn max_errors() -> u32 {
    *MAX_ERRORS.get_or_init(|| {
        std::env::var("VODD_MAX_ERRORS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_MAX_ERRORS)
    })
}
