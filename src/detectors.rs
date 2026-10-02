use crate::address;
use crate::bitcode;

use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::BuildHasherDefault;
use std::hash::Hasher;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

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
                "none" => checks = Checks::NONE,
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
    #[allow(clippy::box_collection)]
    // Boxed: nearly every one is empty, and the deque's fields rode in every
    // return. Worth 8% of a single threaded run.
    pending: Option<Box<VecDeque<Diagnostic>>>,
}

impl DiagnosticList {
    pub fn new() -> DiagnosticList {
        DiagnosticList { pending: None }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.pending
            .get_or_insert_with(Box::default)
            .push_back(diagnostic);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.as_ref().is_none_or(|held| held.is_empty())
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
        self.pending
            .map_or_else(VecDeque::new, |held| *held)
            .into_iter()
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

pub fn checked_shared<T, E: From<address::Invalid>>(
    storage: &address::SharedStorage,
    checks: Checks,
    access: Access,
    apply: impl FnOnce(&[AtomicU8]) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<(T, DiagnosticList), E> {
    let mut diagnostics = DiagnosticList::new();

    let reason = match storage.slice(access.address, access.size, access.atomic) {
        Ok(slots) => return Ok((apply(&slots)?, diagnostics)),
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
    storage: &address::UnsafeSharedRawPtrStorage<Permissions>,
    checks: Checks,
    access: Access,
    update: impl FnOnce(&mut [u8]) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<(T, DiagnosticList), E> {
    let mut diagnostics = DiagnosticList::new();

    let outcome = storage.write_with_metadata(
        access.address,
        access.size,
        access.atomic,
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

#[derive(Debug)]
pub struct BarrierDetector {
    checks: Checks,
    arrivals: Vec<Mutex<Option<Barrier>>>,
    parked: Vec<AtomicBool>,
    pending: Mutex<Option<bitcode::MemorySemantics>>,
}

impl BarrierDetector {
    pub fn new(checks: Checks, lanes: usize) -> BarrierDetector {
        BarrierDetector {
            checks,
            arrivals: (0..lanes).map(|_| Mutex::new(None)).collect(),
            parked: (0..lanes).map(|_| AtomicBool::new(false)).collect(),
            pending: Mutex::new(None),
        }
    }

    pub fn waiting(&self) -> usize {
        self.parked
            .iter()
            .filter(|waiting| waiting.load(Relaxed))
            .count()
    }

    pub fn fold(&self) -> Option<bitcode::MemorySemantics> {
        self.pending.lock().expect("barriers").take()
    }

    pub fn release(&self, lane: usize) -> bool {
        self.parked[lane].swap(false, Relaxed)
    }

    pub fn arrived(&self, entity: Entity, reached: Barrier) {
        self.parked[entity.lane].store(true, Relaxed);
        *self.pending.lock().expect("barriers") = Some(reached.semantics);

        if self.checks.divergence {
            *self.arrivals[entity.lane].lock().expect("barriers") = Some(reached);
        }
    }

    pub fn released(&self) -> Vec<(Option<usize>, Diagnostic)> {
        if !self.checks.divergence {
            return Vec::new();
        }

        let arrivals: Vec<(usize, Barrier)> = self
            .arrivals
            .iter()
            .enumerate()
            .filter_map(|(lane, slot)| Some((lane, slot.lock().expect("barriers").take()?)))
            .collect();

        let expected = arrivals.first().map(|(_, barrier)| *barrier);
        let (arrived, total) = (self.waiting(), self.parked.len());

        arrivals
            .iter()
            .filter(|(_, reached)| Some(reached) != expected.as_ref())
            .map(|(lane, reached)| {
                (
                    Some(*lane),
                    Diagnostic::new(
                        Kind::BarrierDivergence {
                            reached: *reached,
                            expected: expected.expect("an expected barrier"),
                        },
                        Severity::Error,
                    )
                    .at(reached.location),
                )
            })
            .chain((arrived != total).then(|| {
                (
                    None,
                    Diagnostic::new(
                        Kind::BarrierParticipation { arrived, total },
                        Severity::Error,
                    )
                    .at(expected.and_then(|barrier| barrier.location)),
                )
            }))
            .collect()
    }
}

#[derive(Debug)]
pub struct RaceDetector {
    checks: Checks,
    // Apart, so a sync drains its own map whole. Telling them apart by walking
    // every entry was 19% of a run.
    local: Vec<Lane>,
    global: Vec<Lane>,
    // Sharded on the same split the fold uses, so carrying into them is per
    // shard work rather than one serial pass over every address.
    group_global: Vec<Mutex<AddressMap<Record>>>,
    kernel_global: Vec<Mutex<AddressMap<Record>>>,
    // What the last fold of each shard came to, so the next one allocates once
    // instead of growing into place. Every group touches the same spread of
    // addresses, so the previous size is a good guess at the next. Took 7% off
    // a single threaded run.
    fold_sizes: Vec<AtomicUsize>,
    names: Vec<Mutex<Names>>,
    generation: AtomicU32,
    group: [AtomicU64; 3],
    uniform_writes: bool,
}

impl RaceDetector {
    pub fn new(checks: Checks, lanes: usize) -> RaceDetector {
        let maps = |count: usize| {
            (0..count)
                .map(|_| Mutex::new(AddressMap::default()))
                .collect()
        };
        let per_lane = || (0..lanes).map(|_| maps(SHARDS)).collect();

        RaceDetector {
            checks,
            local: per_lane(),
            global: per_lane(),
            group_global: maps(SHARDS),
            kernel_global: maps(SHARDS),
            fold_sizes: (0..SHARDS).map(|_| AtomicUsize::new(0)).collect(),
            names: (0..lanes).map(|_| Mutex::new(Names::default())).collect(),
            generation: AtomicU32::new(0),
            group: [const { AtomicU64::new(0) }; 3],
            uniform_writes: checks.uniform_writes,
        }
    }

    fn lanes(&self, space: address::Region) -> Option<&Vec<Lane>> {
        match space {
            address::Region::Global => Some(&self.global),
            address::Region::Local => Some(&self.local),
            _ => None,
        }
    }

    fn group(&self) -> [u64; 3] {
        core::array::from_fn(|axis| self.group[axis].load(Relaxed))
    }

    pub fn begin_group(&self, group: [u64; 3]) {
        self.generation.fetch_add(1, Relaxed);

        self.group
            .iter()
            .zip(group)
            .for_each(|(axis, value)| axis.store(value, Relaxed));

        for shard in self.local.iter().chain(&self.global).flatten() {
            shard.lock().expect("races").clear();
        }

        for shard in &self.group_global {
            shard.lock().expect("races").clear();
        }
    }

    pub fn record(&self, entity: Entity, access: Access, data: &[u8]) {
        if !self.checks.races {
            return;
        }

        let Access { address, size, store, atomic, location, .. } = access;

        let Some(lanes) = address::region(address).and_then(|space| self.lanes(space)) else {
            return;
        };

        let Some(lane) = lanes.get(entity.lane) else {
            return;
        };

        let Some(names) = self.names.get(entity.lane) else {
            return;
        };

        let generation = self.generation.load(Relaxed) as u16;

        let (named_entity, named_site) = {
            let mut names = names.lock().expect("races");

            (names.entity(generation, entity), names.site(location))
        };

        let mut mark = Mark {
            lane: entity.lane as u16,
            entity: named_entity,
            site: named_site,
            data: 0,
            store,
            atomic,
        };

        // One lock per granule the access spans, so an aligned scalar still
        // takes only the one it used to.
        let end = address + size as u64;
        let mut index = 0;

        while index < size {
            let first = address + index as u64;
            let stop = ((first | GRANULE_MASK) + 1).min(end);
            let mut map = lane[shard_of(first >> GRANULE_BITS)].lock().expect("races");

            for at in first..stop {
                mark.data = data.get(index).copied().unwrap_or(0);

                map.entry(at).or_default().insert(mark);

                index += 1;
            }
        }
    }

    fn named(&self, mark: Mark) -> (Entity, Option<bitcode::Location>) {
        let names = self.names[mark.lane as usize].lock().expect("races");

        (
            names.entities[mark.entity as usize].unwrap_or_default(),
            names.sites[mark.site as usize],
        )
    }

    fn race_diagnostic(
        &self,
        region: address::Region,
        address: u64,
        a: Mark,
        b: Mark,
    ) -> Diagnostic {
        let (first, first_at) = self.named(a);
        let (second, second_at) = self.named(b);

        let describe = |mark: Mark, at: Option<bitcode::Location>| Access {
            region: Some(region),
            address,
            size: 1,
            store: mark.store,
            atomic: mark.atomic,
            location: at,
            entity: None,
        };

        let mut diagnostic = Diagnostic::new(Kind::DataRace { first, second }, Severity::Error)
            .at(first_at)
            .touching(describe(a, first_at));

        diagnostic.partner = Some(describe(b, second_at));

        diagnostic
    }

    pub fn barrier(&self, semantics: bitcode::MemorySemantics) -> Vec<Diagnostic> {
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

    // One task per address shard, folded in lane order within a shard so the
    // reported pairs do not change. Took twelve thread scaling from 3.1x to
    // 6.2x.
    fn sync(&self, space: address::Region) -> Vec<Diagnostic> {
        let uniform_writes = self.uniform_writes;
        let global = space == address::Region::Global;

        let Some(lanes) = self.lanes(space) else {
            return Vec::new();
        };

        let folded: Vec<Vec<Raced>> = (0..SHARDS)
            .into_par_iter()
            .map(|shard| {
                // Sized from the last fold rather than grown from empty. A fold
                // holds every byte the group touched, so growing into one
                // rehashed a quarter of a million entries a group, 14% of a
                // checks-on run.
                let hint = self.fold_sizes[shard].load(Relaxed);
                let mut merged: AddressMap<Record> =
                    AddressMap::with_capacity_and_hasher(hint, Default::default());
                let mut raced: Vec<Raced> = Vec::new();

                for lane in lanes {
                    let mut map = lane[shard].lock().expect("races");

                    for (address, record) in map.drain() {
                        let held = merged.entry(address).or_default();

                        for (a, b) in pairs(record, *held).into_iter().flatten() {
                            if races(a, b, uniform_writes) {
                                insert_race(&mut raced, space, address, a, b);
                            }
                        }

                        held.merge(record);
                    }
                }

                self.fold_sizes[shard].store(merged.len(), Relaxed);

                if global && !merged.is_empty() {
                    let mut carry = self.group_global[shard].lock().expect("races");

                    carry.reserve(merged.len());

                    for (address, record) in merged {
                        carry.entry(address).or_default().merge(record);
                    }
                }

                raced
            })
            .collect();

        let mut raced: Vec<Raced> = Vec::new();

        for found in folded {
            for (region, address, a, b) in found {
                insert_race(&mut raced, region, address, a, b);
            }
        }

        raced
            .into_iter()
            .map(|(region, address, a, b)| self.race_diagnostic(region, address, a, b))
            .collect()
    }

    pub fn end_group(&self) -> Vec<Diagnostic> {
        if !self.checks.races {
            return Vec::new();
        }

        let group = self.group();

        let mut diagnostics = self.sync(address::Region::Local);
        diagnostics.extend(self.sync(address::Region::Global));

        let uniform_writes = self.uniform_writes;
        let generation = self.generation.load(Relaxed) as u16;
        let found: Vec<Vec<Raced>> = (0..SHARDS)
            .into_par_iter()
            .map(|shard| {
                let merged = std::mem::take(&mut *self.group_global[shard].lock().expect("races"));

                if merged.is_empty() {
                    return Vec::new();
                }

                let mut kernel_global = self.kernel_global[shard].lock().expect("races");
                let mut raced: Vec<Raced> = Vec::new();

                // One resize for the group instead of a doubling part way through
                // it. Sizing it for the whole launch up front was 12% slower: the
                // map stays mostly empty until the last few groups, and probing a
                // sparse table that large misses cache on nearly every access.
                kernel_global.reserve(merged.len());

                for (address, record) in merged {
                    let entry = kernel_global.entry(address).or_default();

                    for (a, b) in pairs(record, *entry).into_iter().flatten() {
                        if b.entity != generation && races(a, b, uniform_writes) {
                            insert_race(&mut raced, address::Region::Global, address, a, b);
                        }
                    }

                    entry.merge(record);
                }

                raced
            })
            .collect();

        let mut raced: Vec<Raced> = Vec::new();

        for shard in found {
            for (region, address, a, b) in shard {
                insert_race(&mut raced, region, address, a, b);
            }
        }

        diagnostics.extend(
            raced
                .into_iter()
                .map(|(region, address, a, b)| self.race_diagnostic(region, address, a, b)),
        );

        diagnostics
    }

    pub fn end_kernel(&self) {
        for shard in &self.kernel_global {
            shard.lock().expect("races").clear();
        }
    }
}

// These maps are keyed by address and written once per byte of every access,
// so the hash is on the hot path and does not need to resist attack.
#[derive(Default)]
struct AddressHasher(u64);

impl Hasher for AddressHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        let mut spread = value ^ self.0;

        spread ^= spread >> 33;
        spread = spread.wrapping_mul(0xff51_afd7_ed55_8ccd);
        spread ^= spread >> 33;
        spread = spread.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        spread ^= spread >> 33;

        self.0 = spread;
    }
}

type AddressMap<V> = HashMap<u64, V, BuildHasherDefault<AddressHasher>>;
type Lane = Vec<Mutex<AddressMap<Record>>>;

// Eight measured best on twelve cores: more slows recording, fewer starves the
// fold. Must stay a power of two for the mask below.
const SHARDS: usize = 8;
const SHARD_MASK: u64 = SHARDS as u64 - 1;

const _: () = assert!(SHARDS.is_power_of_two());

// Bytes that share a shard, so a byte lands in the same one whichever access
// reached it; taking it from the access base lost races between a four byte
// and a one byte store to the same place. Four measured alike and sixteen 1.3%
// slower, and eight keeps an aligned scalar of any width to a single lock.
const GRANULE_BITS: u32 = 3;
const GRANULE_MASK: u64 = (1 << GRANULE_BITS) - 1;

fn shard_of(address: u64) -> usize {
    let mut hasher = AddressHasher::default();

    hasher.write_u64(address);

    (hasher.finish() & SHARD_MASK) as usize
}

// One per byte of every access, so its size is the cost of race checking.
// Holding the work item and location inline came to 224 bytes and put a 24 KB
// buffer under 1.9 GB of bookkeeping; interning both took a run 23% faster and
// peak memory to 297 MB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    lane: u16,
    entity: u16,
    site: u16,
    data: u8,
    store: bool,
    atomic: bool,
}

// Interned per lane, so no shared lock. Never cleared between groups, because
// a mark carried into the kernel wide map outlives the group that made it.
#[derive(Debug, Default)]
struct Names {
    entities: Vec<Option<Entity>>,
    sites: Vec<Option<bitcode::Location>>,
}

impl Names {
    fn entity(&mut self, generation: u16, entity: Entity) -> u16 {
        let slot = generation as usize;

        if self.entities.len() <= slot {
            self.entities.resize(slot + 1, None);
        }

        self.entities[slot] = Some(entity);

        generation
    }

    fn site(&mut self, site: Option<bitcode::Location>) -> u16 {
        Names::intern(&mut self.sites, site)
    }

    fn intern<T: PartialEq>(table: &mut Vec<T>, value: T) -> u16 {
        if let Some(found) = table.iter().rposition(|held| *held == value) {
            return found as u16;
        }

        table.push(value);

        (table.len() - 1) as u16
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Record {
    load: Option<Mark>,
    store: Option<Mark>,
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

    fn insert(&mut self, mark: Mark) {
        let slot = match mark.store {
            true => &mut self.store,
            false => &mut self.load,
        };

        if slot.is_none_or(|held| held.atomic) {
            *slot = Some(mark);
        }
    }
}

fn races(a: Mark, b: Mark, uniform_writes: bool) -> bool {
    if a.lane == b.lane && a.entity == b.entity {
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

fn pairs(a: Record, b: Record) -> [Option<(Mark, Mark)>; 3] {
    [
        a.load.zip(b.store),
        a.store.zip(b.load),
        a.store.zip(b.store),
    ]
}

type Raced = (address::Region, u64, Mark, Mark);

fn insert_race(raced: &mut Vec<Raced>, region: address::Region, address: u64, a: Mark, b: Mark) {
    fn alike(a: Mark, b: Mark) -> bool {
        a.lane == b.lane
            && a.entity == b.entity
            && a.site == b.site
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
