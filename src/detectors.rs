use crate::address;
use crate::bitcode;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::OnceLock;

pub use crate::address::Invalid;

static CHECKS: OnceLock<Result<Checks, EnvError>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvError {
    NotUnicode(&'static str),
    UnknownCheck(String),
}

fn variable(name: &'static str) -> Result<Option<String>, EnvError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(EnvError::NotUnicode(name)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Checks {
    pub memory: bool,
    pub types: bool,
    pub divergence: bool,
    pub races: bool,
    pub uniform_writes: bool,
}

impl Checks {
    pub const NONE: Checks = Checks {
        memory: false,
        types: false,
        divergence: false,
        races: false,
        uniform_writes: false,
    };

    pub const ALL: Checks = Checks {
        memory: true,
        types: true,
        divergence: true,
        races: true,
        uniform_writes: false,
    };

    pub fn enabled(&self) -> bool {
        self.memory || self.types || self.divergence || self.races
    }

    pub fn from_env() -> Result<Checks, EnvError> {
        CHECKS
            .get_or_init(|| match variable("VODD_CHECK")? {
                Some(requested) => Checks::parse(&requested),
                None => Ok(Checks::NONE),
            })
            .clone()
    }

    pub fn parse(requested: &str) -> Result<Checks, EnvError> {
        let mut checks = Checks::NONE;

        for name in requested.split(',').map(str::trim) {
            match name {
                "" => {}
                "all" => checks = Checks { uniform_writes: checks.uniform_writes, ..Checks::ALL },
                "mem" => checks.memory = true,
                "type" => checks.types = true,
                "divergence" => checks.divergence = true,
                "races" => checks.races = true,
                "uniform-writes" => checks.uniform_writes = true,
                unknown => return Err(EnvError::UnknownCheck(unknown.to_string())),
            }
        }

        Ok(checks)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Entity {
    pub global: [u64; 3],
    pub local: [u64; 3],
    pub group: [u64; 3],
    pub lane: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access {
    pub region: Option<address::Region>,
    pub address: u64,
    pub size: usize,
    pub store: bool,
    pub atomic: bool,
    pub location: Option<bitcode::Location>,
    pub entity: Option<Entity>,
}

impl Access {
    pub fn new(
        address: u64,
        size: usize,
        store: bool,
        atomic: bool,
        location: Option<bitcode::Location>,
    ) -> Access {
        Access {
            region: address::region(address),
            address,
            size,
            store,
            atomic,
            location,
            entity: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Barrier {
    pub execution_scope: u64,
    pub semantics: bitcode::MemorySemantics,
    pub location: Option<bitcode::Location>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    InvalidAccess { reason: Invalid },
    AccessFlags { writable: bool },
    MappedRegion,
    ArrayIndex { index: u64, count: u64 },
    Misaligned { alignment: usize },
    AtomicWidth { width: u32 },
    BarrierDivergence { reached: Barrier, expected: Barrier },
    BarrierParticipation { arrived: usize, total: usize },
    DataRace { first: Entity, second: Entity },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: Kind,
    pub severity: Severity,
    pub location: Option<bitcode::Location>,
    pub entity: Option<Entity>,
    pub access: Option<Access>,
    pub partner: Option<Access>,
}

impl Diagnostic {
    pub fn new(kind: Kind, severity: Severity) -> Diagnostic {
        Diagnostic {
            kind,
            severity,
            location: None,
            entity: None,
            access: None,
            partner: None,
        }
    }

    pub fn at(mut self, location: Option<bitcode::Location>) -> Diagnostic {
        self.location = location;
        self
    }

    pub fn touching(mut self, access: Access) -> Diagnostic {
        self.access = Some(access);
        self
    }

    pub fn by(mut self, entity: Entity) -> Diagnostic {
        self.entity = Some(entity);

        if let Some(access) = self.access.as_mut()
            && access.entity.is_none()
        {
            access.entity = Some(entity);
        }

        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticList {
    pending: VecDeque<Diagnostic>,
}

impl DiagnosticList {
    pub fn new() -> DiagnosticList {
        DiagnosticList { pending: VecDeque::new() }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.pending.push_back(diagnostic);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl From<Diagnostic> for DiagnosticList {
    fn from(diagnostic: Diagnostic) -> DiagnosticList {
        let mut list = DiagnosticList::new();
        list.push(diagnostic);
        list
    }
}

impl Extend<Diagnostic> for DiagnosticList {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, diagnostics: I) {
        for diagnostic in diagnostics {
            self.push(diagnostic);
        }
    }
}

impl IntoIterator for DiagnosticList {
    type Item = Diagnostic;
    type IntoIter = std::collections::vec_deque::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.pending.into_iter()
    }
}

pub fn invalid_access(checks: Checks, access: Access, reason: Invalid) -> Option<Diagnostic> {
    if !checks.memory {
        return None;
    }

    Some(Diagnostic::new(Kind::InvalidAccess { reason }, Severity::Error).touching(access))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub at: usize,
    pub size: usize,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permissions {
    pub reads: bool,
    pub writes: bool,
    pub mappings: Vec<Mapping>,
}

pub fn access_flags(
    checks: Checks,
    access: Access,
    permissions: &Permissions,
) -> Option<Diagnostic> {
    if !checks.memory {
        return None;
    }

    let reads = access.atomic || !access.store;
    let writes = access.atomic || access.store;

    let writable = if reads && !permissions.reads {
        true
    } else if writes && !permissions.writes {
        false
    } else {
        return None;
    };

    Some(Diagnostic::new(Kind::AccessFlags { writable }, Severity::Error).touching(access))
}

pub fn mapped_region(
    checks: Checks,
    access: Access,
    offset: u64,
    permissions: &Permissions,
) -> Option<Diagnostic> {
    if !checks.memory {
        return None;
    }

    let blocked = permissions.mappings.iter().any(|mapping| {
        let overlaps = offset < (mapping.at + mapping.size) as u64
            && (mapping.at as u64) < offset + access.size as u64;

        overlaps && (access.store || mapping.writable)
    });

    if !blocked {
        return None;
    }

    Some(Diagnostic::new(Kind::MappedRegion, Severity::Error).touching(access))
}

pub fn element_index(checks: Checks, index: u64, count: Option<u64>) -> Option<Diagnostic> {
    if !checks.types {
        return None;
    }

    let count = count?;

    if index < count {
        return None;
    }

    Some(Diagnostic::new(
        Kind::ArrayIndex { index, count },
        Severity::Error,
    ))
}

pub fn alignment(
    checks: Checks,
    access: Access,
    declared: Option<u32>,
    natural: usize,
) -> Option<Diagnostic> {
    if !checks.memory {
        return None;
    }

    let required = match declared {
        Some(declared) if declared != 0 => declared as usize,
        _ => natural,
    };

    if required == 0 || access.address.is_multiple_of(required as u64) {
        return None;
    }

    Some(
        Diagnostic::new(Kind::Misaligned { alignment: required }, Severity::Error).touching(access),
    )
}

pub fn atomic_width(checks: Checks, access: Access, width: u32) -> Option<Diagnostic> {
    if !checks.memory {
        return None;
    }

    if width != 32 && width != 64 {
        return Some(Diagnostic::new(
            Kind::AtomicWidth { width },
            Severity::Error,
        ));
    }

    alignment(checks, access, None, (width / 8) as usize)
}

pub fn checked_read<T, E: From<address::Invalid>>(
    storage: &address::Storage,
    checks: Checks,
    access: Access,
    decode: impl FnOnce(&[u8]) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<(T, DiagnosticList), E> {
    let mut diagnostics = DiagnosticList::new();

    let reason = match storage.read(access.address, access.size) {
        Ok(bytes) => return Ok((decode(bytes)?, diagnostics)),
        Err(reason) => reason,
    };

    if !checks.memory {
        return Err(reason.into());
    }

    diagnostics.extend(invalid_access(checks, access, reason));

    Ok((fallback()?, diagnostics))
}

pub fn checked_write<T, E: From<address::Invalid>>(
    storage: &mut address::Storage,
    checks: Checks,
    access: Access,
    update: impl FnOnce(&mut [u8]) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<(T, DiagnosticList), E> {
    let mut diagnostics = DiagnosticList::new();

    let reason = match storage.write(access.address, access.size) {
        Ok(destination) => return Ok((update(destination)?, diagnostics)),
        Err(reason) => reason,
    };

    if !checks.memory {
        return Err(reason.into());
    }

    diagnostics.extend(invalid_access(checks, access, reason));

    Ok((fallback()?, diagnostics))
}

pub fn checked_buffer_read<T, E: From<address::Invalid>>(
    storage: &address::UnsafeSharedRawPtrStorage<Permissions>,
    checks: Checks,
    access: Access,
    decode: impl FnOnce(&[u8]) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<(T, DiagnosticList), E> {
    let mut diagnostics = DiagnosticList::new();

    let outcome =
        storage.read_with_metadata(access.address, access.size, |permissions, offset, bytes| {
            diagnostics.extend(access_flags(checks, access, permissions));
            diagnostics.extend(mapped_region(checks, access, offset, permissions));

            decode(bytes)
        });

    match outcome {
        Ok(decoded) => Ok((decoded?, diagnostics)),
        Err(reason) if !checks.memory => Err(reason.into()),
        Err(reason) => {
            diagnostics.extend(invalid_access(checks, access, reason));

            Ok((fallback()?, diagnostics))
        }
    }
}

pub fn checked_buffer_write<T, E: From<address::Invalid>>(
    storage: &mut address::UnsafeSharedRawPtrStorage<Permissions>,
    checks: Checks,
    access: Access,
    update: impl FnOnce(&mut [u8]) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<(T, DiagnosticList), E> {
    let mut diagnostics = DiagnosticList::new();

    let outcome = storage.write_with_metadata(
        access.address,
        access.size,
        |permissions, offset, destination| {
            diagnostics.extend(access_flags(checks, access, permissions));
            diagnostics.extend(mapped_region(checks, access, offset, permissions));

            update(destination)
        },
    );

    match outcome {
        Ok(updated) => Ok((updated?, diagnostics)),
        Err(reason) if !checks.memory => Err(reason.into()),
        Err(reason) => {
            diagnostics.extend(invalid_access(checks, access, reason));

            Ok((fallback()?, diagnostics))
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BarrierDetector {
    checks: Checks,
    expected: Option<Barrier>,
    parked: Vec<bool>,
    pending: Option<bitcode::MemorySemantics>,
}

impl BarrierDetector {
    pub fn new(checks: Checks, lanes: usize) -> BarrierDetector {
        BarrierDetector {
            checks,
            expected: None,
            parked: vec![false; lanes],
            pending: None,
        }
    }

    pub fn waiting(&self) -> usize {
        self.parked.iter().filter(|waiting| **waiting).count()
    }

    pub fn fold(&mut self) -> Option<bitcode::MemorySemantics> {
        self.pending.take()
    }

    pub fn release(&mut self, lane: usize) -> bool {
        std::mem::replace(&mut self.parked[lane], false)
    }

    pub fn arrived(&mut self, entity: Entity, reached: Barrier) -> Option<Diagnostic> {
        self.parked[entity.lane] = true;
        self.pending = Some(reached.semantics);

        if !self.checks.divergence {
            return None;
        }

        let Some(expected) = self.expected else {
            self.expected = Some(reached);
            return None;
        };

        if reached == expected {
            return None;
        }

        Some(
            Diagnostic::new(
                Kind::BarrierDivergence { reached, expected },
                Severity::Error,
            )
            .at(reached.location),
        )
    }

    pub fn released(&mut self) -> Option<Diagnostic> {
        if !self.checks.divergence {
            return None;
        }

        let expected = self.expected.take();
        let (arrived, total) = (self.waiting(), self.parked.len());

        if arrived == total {
            return None;
        }

        Some(
            Diagnostic::new(
                Kind::BarrierParticipation { arrived, total },
                Severity::Error,
            )
            .at(expected.and_then(|barrier| barrier.location)),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct RaceDetector {
    checks: Checks,
    lanes: Vec<HashMap<u64, Record>>,
    group_global: HashMap<u64, Record>,
    kernel_global: HashMap<u64, Record>,
    group: [u64; 3],
    uniform_writes: bool,
}

impl RaceDetector {
    pub fn new(checks: Checks, lanes: usize) -> RaceDetector {
        RaceDetector {
            checks,
            lanes: vec![HashMap::new(); lanes],
            group_global: HashMap::new(),
            kernel_global: HashMap::new(),
            group: [0, 0, 0],
            uniform_writes: checks.uniform_writes,
        }
    }

    pub fn begin_group(&mut self, group: [u64; 3]) {
        self.group = group;

        for lane in &mut self.lanes {
            lane.clear();
        }

        self.group_global.clear();
    }

    pub fn record(&mut self, entity: Entity, access: Access, data: &[u8]) {
        if !self.checks.races {
            return;
        }

        let Access { address, size, store, atomic, location, .. } = access;
        let space = address::region(address);

        if !matches!(
            space,
            Some(address::Region::Global) | Some(address::Region::Local)
        ) {
            return;
        }

        let Some(map) = self.lanes.get_mut(entity.lane) else {
            return;
        };

        for index in 0..size {
            let access = RaceAccess {
                entity,
                location,
                store,
                atomic,
                data: data.get(index).copied().unwrap_or(0),
            };

            map.entry(address + index as u64)
                .or_default()
                .insert(access);
        }
    }

    pub fn barrier(&mut self, semantics: bitcode::MemorySemantics) -> Vec<Diagnostic> {
        if !self.checks.races {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();

        match semantics.space {
            bitcode::MemorySpace::None => {}
            bitcode::MemorySpace::Workgroup => {
                diagnostics.extend(self.sync(address::Region::Local));
            }
            bitcode::MemorySpace::CrossWorkgroup => {
                diagnostics.extend(self.sync(address::Region::Global));
            }
            bitcode::MemorySpace::Both => {
                diagnostics.extend(self.sync(address::Region::Local));
                diagnostics.extend(self.sync(address::Region::Global));
            }
        }

        diagnostics
    }

    fn sync(&mut self, space: address::Region) -> Vec<Diagnostic> {
        let RaceDetector { lanes, group_global, uniform_writes, .. } = self;
        let uniform_writes = *uniform_writes;
        let global = space == address::Region::Global;

        let mut raced = Vec::new();
        let mut merged: HashMap<u64, Record> = HashMap::new();

        for map in lanes.iter_mut() {
            for (address, record) in
                map.extract_if(|address, _| address::region(*address) == Some(space))
            {
                let held = merged.entry(address).or_default();

                for (a, b) in pairs(record, *held).into_iter().flatten() {
                    if races(a, b, uniform_writes) {
                        insert_race(&mut raced, space, address, a, b);
                    }
                }

                held.merge(record);

                if global {
                    group_global.entry(address).or_default().merge(record);
                }
            }
        }

        raced
            .into_iter()
            .map(|(region, address, a, b)| race_diagnostic(region, address, a, b))
            .collect()
    }

    pub fn end_group(&mut self) -> Vec<Diagnostic> {
        if !self.checks.races {
            return Vec::new();
        }

        let mut diagnostics = self.sync(address::Region::Local);
        diagnostics.extend(self.sync(address::Region::Global));

        let merged = std::mem::take(&mut self.group_global);
        let mut raced = Vec::new();

        for (address, record) in merged {
            let held = self.kernel_global.entry(address).or_default();

            for (a, b) in pairs(record, *held).into_iter().flatten() {
                if b.entity.group != self.group && races(a, b, self.uniform_writes) {
                    insert_race(&mut raced, address::Region::Global, address, a, b);
                }
            }

            held.merge(record);
        }

        diagnostics.extend(
            raced
                .into_iter()
                .map(|(region, address, a, b)| race_diagnostic(region, address, a, b)),
        );

        diagnostics
    }

    pub fn end_kernel(&mut self) {
        self.kernel_global.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RaceAccess {
    entity: Entity,
    location: Option<bitcode::Location>,
    store: bool,
    atomic: bool,
    data: u8,
}

#[derive(Debug, Clone, Copy, Default)]
struct Record {
    load: Option<RaceAccess>,
    store: Option<RaceAccess>,
}

impl Record {
    fn merge(&mut self, other: Record) {
        if let Some(load) = other.load {
            self.insert(load);
        }

        if let Some(store) = other.store {
            self.insert(store);
        }
    }

    fn insert(&mut self, access: RaceAccess) {
        let slot = match access.store {
            true => &mut self.store,
            false => &mut self.load,
        };

        if slot.is_none_or(|held| held.atomic) {
            *slot = Some(access);
        }
    }
}

fn races(a: RaceAccess, b: RaceAccess, uniform_writes: bool) -> bool {
    if a.entity == b.entity {
        return false;
    }

    if a.atomic && b.atomic {
        return false;
    }

    if !a.store && !b.store {
        return false;
    }

    if !a.store || !b.store {
        return true;
    }

    uniform_writes || a.data != b.data
}

fn pairs(a: Record, b: Record) -> [Option<(RaceAccess, RaceAccess)>; 3] {
    [
        a.load.zip(b.store),
        a.store.zip(b.load),
        a.store.zip(b.store),
    ]
}

fn insert_race(
    raced: &mut Vec<(address::Region, u64, RaceAccess, RaceAccess)>,
    region: address::Region,
    address: u64,
    a: RaceAccess,
    b: RaceAccess,
) {
    fn alike(a: RaceAccess, b: RaceAccess) -> bool {
        a.entity == b.entity
            && a.location == b.location
            && a.store == b.store
            && a.atomic == b.atomic
    }

    for held in raced.iter_mut() {
        let same = (alike(held.2, a) && alike(held.3, b)) || (alike(held.2, b) && alike(held.3, a));

        if same {
            if address < held.1 {
                *held = (region, address, a, b);
            }

            return;
        }
    }

    raced.push((region, address, a, b));
}

fn race_diagnostic(
    region: address::Region,
    address: u64,
    a: RaceAccess,
    b: RaceAccess,
) -> Diagnostic {
    let describe = |access: RaceAccess| Access {
        region: Some(region),
        address,
        size: 1,
        store: access.store,
        atomic: access.atomic,
        location: access.location,
        entity: None,
    };

    let mut diagnostic = Diagnostic::new(
        Kind::DataRace { first: a.entity, second: b.entity },
        Severity::Error,
    )
    .at(a.location)
    .touching(describe(a));

    diagnostic.partner = Some(describe(b));

    diagnostic
}
