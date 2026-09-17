use crate::bitcode;
use crate::detectors;
use crate::interpreter;

use serde::Serialize;

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Site {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub text: Option<String>,
}

impl fmt::Display for Site {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}:{}", self.file, self.line, self.column)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Life {
    Pending,
    Running,
    Parked,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Uniform,
    Affine,
    Divergent,
    Shared,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub name: String,
    pub at: Site,
    pub shown: Option<String>,
    pub type_name: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub name: String,
    pub at: Option<Site>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Detail {
    pub label: String,
    pub value: String,
}

pub struct Diagnostic {
    pub severity: Severity,
    pub headline: String,
    pub detail: String,
    pub at: Option<Site>,
    pub rows: Vec<Detail>,
    pub blamed: Option<(u64, usize)>,
}

pub struct Lane {
    pub at: Option<Site>,
    pub stack: Vec<Frame>,
    pub values: Vec<Value>,
    pub life: Life,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Fresh,
    Ready,
    Parked,
    Done,
}

impl Progress {
    pub fn resume(self) -> Option<interpreter::Resume> {
        match self {
            Progress::Fresh => Some(interpreter::Resume::Start),
            Progress::Ready => Some(interpreter::Resume::Ack),
            Progress::Parked | Progress::Done => None,
        }
    }

    pub fn is_ready(self) -> bool {
        self.resume().is_some()
    }

    pub fn life(self) -> Life {
        match self {
            Progress::Fresh | Progress::Ready => Life::Running,
            Progress::Parked => Life::Parked,
            Progress::Done => Life::Done,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Brief,
    Detailed,
    Ended,
}

impl Depth {
    pub fn detailed(self) -> bool {
        self != Depth::Brief
    }
}

pub struct Inspector {
    module: Arc<bitcode::Module>,
    source: Option<Arc<String>>,
}

impl Inspector {
    pub fn new(module: Arc<bitcode::Module>, source: Option<Arc<String>>) -> Inspector {
        Inspector { module, source }
    }

    pub fn site(&self, location: Option<bitcode::Location>) -> Option<Site> {
        let location = location?;
        let path = self.module.string(location.file).unwrap_or("<unknown>");
        let file = path.rsplit('/').next().unwrap_or(path);

        Some(Site {
            file: file.to_string(),
            line: location.line,
            column: location.column,
            text: self.text(location.line),
        })
    }

    pub fn text(&self, line: u32) -> Option<String> {
        self.source
            .as_ref()
            .and_then(|source| source.lines().nth(line.saturating_sub(1) as usize))
            .map(str::to_string)
    }

    pub fn frames(&self, item: &interpreter::Interpreter) -> Vec<Frame> {
        item.stack()
            .into_iter()
            .map(|called| Frame { name: called.name, at: self.site(called.at) })
            .collect()
    }

    pub fn lanes(
        &self,
        items: &[interpreter::Interpreter],
        lanes: &[Progress],
        depth: Depth,
    ) -> Result<Vec<Lane>, interpreter::Error> {
        let places: Vec<Option<bitcode::Location>> = items
            .iter()
            .map(interpreter::Interpreter::location)
            .collect();

        let clusters: Vec<(Option<bitcode::Location>, Option<Site>, Vec<usize>)> = places
            .iter()
            .enumerate()
            .filter(|(lane, at)| !places[..*lane].contains(at))
            .map(|(_, at)| {
                let members = places
                    .iter()
                    .enumerate()
                    .filter(|(_, held)| *held == at)
                    .map(|(lane, _)| lane)
                    .collect();

                (*at, self.site(*at), members)
            })
            .collect();

        items
            .iter()
            .enumerate()
            .map(|(lane, item)| {
                let (_, site, members) = clusters
                    .iter()
                    .find(|(held, ..)| *held == places[lane])
                    .expect("every lane is in a cluster");

                let values = match members.first() {
                    Some(first) if depth.detailed() && *first == lane => {
                        self.values(items, members, site.as_ref())?
                    }
                    _ => Vec::new(),
                };

                Ok(Lane {
                    at: site.clone(),
                    stack: self.frames(item),
                    values,
                    life: lanes[lane].life(),
                })
            })
            .collect()
    }

    pub fn values(
        &self,
        items: &[interpreter::Interpreter],
        members: &[usize],
        at: Option<&Site>,
    ) -> Result<Vec<Value>, interpreter::Error> {
        let Some(at) = at else {
            return Ok(Vec::new());
        };

        let readings = members
            .iter()
            .filter_map(|lane| items.get(*lane))
            .map(|item| item.locals())
            .collect::<Result<Vec<Vec<interpreter::Binding>>, interpreter::Error>>()?;

        let Some(named) = readings.first() else {
            return Ok(Vec::new());
        };

        Ok(named
            .iter()
            .enumerate()
            .map(|(slot, local)| {
                let shown: Vec<Option<&String>> = readings
                    .iter()
                    .map(|lane| lane.get(slot).and_then(|entry| entry.value.as_ref()))
                    .collect();

                let uniform = shown.windows(2).all(|pair| pair[0] == pair[1]);

                let kind = match (uniform, local.pointer) {
                    (true, true) => Kind::Shared,
                    (true, false) => Kind::Uniform,
                    (false, _) => match affine(&shown) {
                        true => Kind::Affine,
                        false => Kind::Divergent,
                    },
                };

                Value {
                    name: local.name.clone(),
                    at: Site {
                        file: at.file.clone(),
                        line: local.line,
                        column: 1,
                        text: self.text(local.line),
                    },
                    shown: match uniform {
                        true => local.value.clone(),
                        false => spread(&shown),
                    },
                    type_name: local.type_name.clone(),
                    kind,
                }
            })
            .collect())
    }

    pub fn annotate(
        &self,
        diagnostic: &detectors::Diagnostic,
        item: Option<&interpreter::Interpreter>,
        group: u64,
    ) -> Diagnostic {
        let (stack, locals) = match item {
            Some(item) => (self.frames(item), bindings(item)),
            None => (Vec::new(), Vec::new()),
        };

        let blamed = diagnostic.entity.map(|entity| (group, entity.lane));

        annotated(
            diagnostic,
            self.site(diagnostic.location),
            stack,
            locals,
            blamed,
        )
    }
}

fn annotated(
    diagnostic: &detectors::Diagnostic,
    at: Option<Site>,
    stack: Vec<Frame>,
    locals: Vec<(String, String)>,
    blamed: Option<(u64, usize)>,
) -> Diagnostic {
    let severity = match diagnostic.severity {
        detectors::Severity::Error => Severity::Error,
        detectors::Severity::Warning => Severity::Warning,
    };

    let rows = diagnostic
        .entity
        .into_iter()
        .flat_map(|entity| {
            [
                row("work item", triple(entity.global)),
                row("work group", triple(entity.group)),
                row("lane", entity.lane.to_string()),
            ]
        })
        .chain(about(&diagnostic.kind))
        .chain(diagnostic.access.map(|access| {
            row(
                match access.store {
                    true => "write",
                    false => "read",
                },
                format!("{} bytes at {:#x}", access.size, access.address),
            )
        }))
        .chain(stack.iter().enumerate().map(|(depth, frame)| {
            row(
                &format!("frame {depth}"),
                match &frame.at {
                    Some(site) => format!("{} at {site}", frame.name),
                    None => frame.name.clone(),
                },
            )
        }))
        .chain(locals.into_iter().map(|(name, held)| row(&name, held)))
        .chain(diagnostic.partner.map(|partner| {
            row(
                "partner",
                format!("{} bytes at {:#x}", partner.size, partner.address),
            )
        }))
        .collect();

    Diagnostic {
        severity,
        headline: diagnostic.kind.to_string(),
        detail: diagnostic.to_string(),
        at,
        rows,
        blamed,
    }
}

fn about(kind: &detectors::Kind) -> Vec<Detail> {
    match kind {
        detectors::Kind::DataRace { first, second } => vec![
            row("first item", triple(first.global)),
            row("second item", triple(second.global)),
        ],
        detectors::Kind::ArrayIndex { index, count } => vec![
            row("index", index.to_string()),
            row("elements", count.to_string()),
        ],
        detectors::Kind::Misaligned { alignment } => {
            vec![row("alignment", format!("{alignment} bytes"))]
        }
        detectors::Kind::AtomicWidth { width } => vec![row("width", format!("{width} bits"))],
        detectors::Kind::BarrierParticipation { arrived, total } => vec![
            row("arrived", arrived.to_string()),
            row("expected", total.to_string()),
        ],
        detectors::Kind::BarrierDivergence { reached, expected } => vec![
            row("reached", format!("scope {:#x}", reached.execution_scope)),
            row("expected", format!("scope {:#x}", expected.execution_scope)),
        ],
        detectors::Kind::InvalidAccess { reason } => vec![row("reason", format!("{reason:?}"))],
        detectors::Kind::AccessFlags { writable } => vec![row(
            "buffer",
            match writable {
                true => "write only".to_string(),
                false => "read only".to_string(),
            },
        )],
        detectors::Kind::MappedRegion => vec![row("buffer", "held mapped by the host".to_string())],
    }
}

fn row(label: &str, value: String) -> Detail {
    Detail { label: label.to_string(), value }
}

fn triple(value: [u64; 3]) -> String {
    format!("{}, {}, {}", value[0], value[1], value[2])
}

fn bindings(item: &interpreter::Interpreter) -> Vec<(String, String)> {
    let Ok(held) = item.locals() else {
        return Vec::new();
    };

    held.into_iter()
        .filter_map(|local| Some((local.name, local.value?)))
        .collect()
}

fn affine(shown: &[Option<&String>]) -> bool {
    let held: Vec<i64> = shown
        .iter()
        .filter_map(|slot| slot.and_then(|text| text.parse().ok()))
        .collect();

    if held.len() < 3 || held.len() != shown.len() {
        return false;
    }

    let step = held[1] - held[0];

    step != 0 && held.windows(2).all(|pair| pair[1] - pair[0] == step)
}

fn spread(shown: &[Option<&String>]) -> Option<String> {
    let held: Vec<&String> = shown.iter().flatten().copied().collect();
    let first = held.first()?;
    let last = held.last()?;

    let distinct = held.iter().collect::<BTreeSet<&&String>>().len();

    Some(format!("{first} .. {last}, {distinct} distinct"))
}
