use crate::debugger;
use crate::hypermedia;
use crate::inspect;
use minijinja::AutoEscape;
use minijinja::Environment;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use syntect::html::ClassStyle;
use syntect::html::line_tokens_to_classed_spans;
use syntect::parsing::ParseState;
use syntect::parsing::ScopeStack;
use syntect::parsing::SyntaxSet;

pub const COLOURS: [&str; 8] = [
    "#0b6e99", "#0f7b6c", "#d9730d", "#6940a5", "#ad1a72", "#cb912f", "#e03e3e", "#4dab9a",
];

pub const FRAGMENTS: [Fragment; 8] = [
    Fragment::Groups,
    Fragment::Items,
    Fragment::Source,
    Fragment::Paths,
    Fragment::Side,
    Fragment::Dock,
    Fragment::Dockbar,
    Fragment::Status,
];

pub const SHARED: [Fragment; 1] = [Fragment::Tabs];
pub const STYLE: &str = concat!(
    include_str!("templates/fonts.css"),
    include_str!("templates/style.css")
);
const LATTICE: &str = include_str!("templates/lattice.html");
const UNAVAILABLE: &str = include_str!("templates/unavailable.html");
const SOURCE: &str = include_str!("templates/source.html");
const PATHS: &str = include_str!("templates/paths.html");
const VALUES: &str = include_str!("templates/values.html");
const FINDINGS: &str = include_str!("templates/findings.html");
const STACK: &str = include_str!("templates/stack.html");
const STATUS: &str = include_str!("templates/status.html");
const DETAILS: &str = include_str!("templates/details.html");
const ITEMS: &str = include_str!("templates/items.html");
const SIDE: &str = include_str!("templates/side.html");
const DOCK: &str = include_str!("templates/dock.html");
const EMPTY: &str = include_str!("templates/empty.html");
const PAGE: &str = include_str!("templates/page.html");
const TABS: &str = include_str!("templates/tabs.html");
const DOCKBAR: &str = include_str!("templates/dockbar.html");
const REFUSED: &str = include_str!("templates/refused.html");
const SHELL: &str = include_str!("templates/shell.html");
const RESUME: &str = include_str!("templates/icons/resume.svg");
const STEP: &str = include_str!("templates/icons/step.svg");
const UNPLUGGED: &str = include_str!("templates/icons/unplugged.svg");
const PAGES: &[(&str, &str)] = &[
    ("lattice", LATTICE),
    ("unavailable", UNAVAILABLE),
    ("source", SOURCE),
    ("paths", PATHS),
    ("values", VALUES),
    ("findings", FINDINGS),
    ("stack", STACK),
    ("status", STATUS),
    ("details", DETAILS),
    ("items", ITEMS),
    ("side", SIDE),
    ("dock", DOCK),
    ("empty", EMPTY),
    ("page", PAGE),
    ("tabs", TABS),
    ("dockbar", DOCKBAR),
    ("shell", SHELL),
];

const MAX_SIDE: u64 = 32;
const MAX_CELLS: u64 = MAX_SIDE * MAX_SIDE;
const MAX_PLANES: u64 = 64;
const MAX_LANES: u64 = 256;
const MAX_HIGHLIGHTS: usize = 32;
static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static HIGHLIGHTS: OnceLock<Mutex<HashMap<u64, Arc<Vec<String>>>>> = OnceLock::new();
static ENVIRONMENT: OnceLock<Result<Environment<'static>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fragment {
    Page,
    Tabs,
    Groups,
    Items,
    Source,
    Paths,
    Side,
    Dock,
    Dockbar,
    Status,
}

impl Fragment {
    pub fn name(self) -> &'static str {
        match self {
            Fragment::Page => "page",
            Fragment::Tabs => "tabs",
            Fragment::Groups => "groups",
            Fragment::Items => "items",
            Fragment::Source => "source",
            Fragment::Paths => "paths",
            Fragment::Side => "side",
            Fragment::Dock => "dock",
            Fragment::Dockbar => "dockbar",
            Fragment::Status => "status",
        }
    }

    pub fn named(name: &str) -> Option<Fragment> {
        let all = [
            Fragment::Page,
            Fragment::Tabs,
            Fragment::Groups,
            Fragment::Items,
            Fragment::Source,
            Fragment::Paths,
            Fragment::Side,
            Fragment::Dock,
            Fragment::Dockbar,
            Fragment::Status,
        ];

        all.into_iter().find(|one| one.name() == name)
    }

    pub fn id(self, at: usize) -> String {
        match self {
            Fragment::Page | Fragment::Tabs => self.name().to_string(),
            _ => format!("{}-{at}", self.name()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<minijinja::Error> for Error {
    fn from(error: minijinja::Error) -> Error {
        Error(format!("the template failed, {error}"))
    }
}

#[derive(Serialize)]
struct UnavailableView<'a> {
    id: &'a str,
    title: &'a str,
    reason: String,
}

#[derive(Serialize)]
struct EmptyView<'a> {
    id: &'a str,
    title: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct LatticeCell {
    label: String,
    life: inspect::Life,
    fill: String,
    marked: bool,
    selected: bool,
    tip: String,
    pick: String,
}

#[derive(Serialize)]
struct LatticePlane {
    depth: u64,
    rows: Vec<Vec<LatticeCell>>,
}

#[derive(Serialize)]
struct LatticeView {
    id: String,
    kind: &'static str,
    title: &'static str,
    note: String,
    columns: u64,
    rows: u64,
    dims: u8,
    labelled: bool,
    stacked: bool,
    block: u64,
    stride: u64,
    planes: Vec<LatticePlane>,
}

struct LatticeShape {
    columns: u64,
    rows: u64,
    block: u64,
}

#[derive(Serialize)]
struct SourceRow {
    number: u32,
    stop: bool,
    body: String,
    mark: &'static str,
    gap: u32,
}

#[derive(Serialize)]
struct ControlButton {
    control: debugger::Control,
    label: &'static str,
    icon: &'static str,
    enabled: bool,
    send: String,
}

#[derive(Serialize)]
struct ControlBarView {
    state: debugger::State,
    state_label: &'static str,
    commands: Vec<ControlButton>,
}

#[derive(Serialize)]
struct SourceView {
    id: String,
    file: String,
    rows: Vec<SourceRow>,
    controls: ControlBarView,
}

#[derive(Serialize)]
struct PathRow {
    name: String,
    colour: String,
    label: String,
    tip: String,
    items: String,
    share: String,
    current: bool,
    pick: String,
}

#[derive(Serialize)]
struct PathsView {
    id: String,
    note: String,
    tracks: Vec<PathRow>,
}

#[derive(Serialize)]
struct ValueRow {
    name: String,
    at: String,
    shown: String,
    tone: &'static str,
    type_name: String,
    kind: inspect::Kind,
}

#[derive(Serialize)]
struct ValuesView {
    id: String,
    scope: String,
    values: Vec<ValueRow>,
}

#[derive(Serialize)]
struct FrameRow {
    name: String,
    tone: &'static str,
    at: String,
    text: String,
    selected: bool,
    depth: usize,
}

#[derive(Serialize)]
struct CallStackView {
    id: String,
    note: String,
    frames: Vec<FrameRow>,
}

#[derive(Serialize)]
struct FindingRow {
    severity: inspect::Severity,
    headline: String,
    detail: String,
    at: String,
    selected: bool,
    pick: String,
}

#[derive(Serialize)]
struct FindingsView {
    id: String,
    findings: Vec<FindingRow>,
}

#[derive(Serialize)]
struct DetailRow {
    label: String,
    value: String,
}

#[derive(Serialize)]
struct DetailsView<'a> {
    id: &'a str,
    rows: Vec<DetailRow>,
}

#[derive(Serialize)]
struct ItemsView {
    id: String,
    body: String,
}

#[derive(Serialize)]
struct SideView {
    id: String,
    values: String,
    stack: String,
}

#[derive(Serialize)]
struct DockView {
    id: String,
    findings: String,
    details: String,
}

#[derive(Serialize)]
struct StatusBarView {
    id: String,
    state: debugger::State,
    state_label: &'static str,
    global: String,
    local: String,
    progress: String,
    problems: String,
    severity: &'static str,
}

#[derive(Serialize)]
struct PaneView {
    at: usize,
    current: bool,
    status: String,
    dockbar: String,

    source: String,
    groups: String,
    paths: String,

    items: String,
    side: String,
    dock: String,
}

#[derive(Serialize)]
struct TabRow {
    at: usize,
    name: String,
    state: debugger::State,
    current: bool,
}

#[derive(Serialize)]
struct TabsView {
    id: String,
    tabs: Vec<TabRow>,
}

#[derive(Serialize)]
struct DockbarView {
    id: String,
    problems: String,
    severity: &'static str,
}

#[derive(Serialize)]
struct PageView {
    strip: String,
    panes: Vec<PaneView>,
}

#[derive(Serialize)]
struct ShellView {
    body: String,
    scripts: &'static str,
    unplugged: &'static str,
}

type Result<T> = core::result::Result<T, Error>;

pub fn shell(view: &debugger::Page) -> String {
    let held = ShellView {
        body: fragment(view, Fragment::Page, 0),
        scripts: hypermedia::SCRIPTS,
        unplugged: UNPLUGGED,
    };

    paint("shell", &held).unwrap_or_else(|error| unavailable("", "Shell", &error))
}

pub fn fragment(view: &debugger::Page, want: Fragment, at: usize) -> String {
    let id = want.id(at);
    let pane = view.tabs.get(at).zip(view.focus.get(at));

    let drawn = match (want, pane) {
        (Fragment::Page, _) => page(view),
        (Fragment::Tabs, _) => tabs(view, &id),
        (_, None) => Err(missing(&id)),
        (Fragment::Groups, Some((pane, focus))) => groups(pane, focus, &id),
        (Fragment::Items, Some((pane, focus))) => items(pane, focus, at),
        (Fragment::Source, Some((pane, focus))) => source(pane, focus, &id),
        (Fragment::Paths, Some((pane, focus))) => paths(pane, focus, &id),
        (Fragment::Side, Some((pane, focus))) => side(pane, focus, at),
        (Fragment::Dock, Some((pane, focus))) => dock(pane, focus, at),
        (Fragment::Dockbar, Some((pane, _))) => dockbar(pane, &id),
        (Fragment::Status, Some((pane, _))) => status_bar(pane, &id),
    };

    drawn.unwrap_or_else(|error| unavailable(&id, want.name(), &error))
}

fn page(view: &debugger::Page) -> Result<String> {
    if view.tabs.is_empty() {
        return refuse("the page has no tabs, there is nothing to show");
    }

    if view.tabs.len() != view.focus.len() {
        return refuse(format!(
            "{} launches but {} reactivity entries, they pair one to one",
            view.tabs.len(),
            view.focus.len()
        ));
    }

    if view.selected >= view.tabs.len() {
        return refuse(format!(
            "tab {} is selected but the page only has {}",
            view.selected,
            view.tabs.len()
        ));
    }

    let mut panes = Vec::new();

    for (at, launch) in view.tabs.iter().enumerate() {
        panes.push(pane(launch, &view.focus[at], at, at == view.selected)?);
    }

    paint(
        "page",
        &PageView { strip: tabs(view, &Fragment::Tabs.id(0))?, panes },
    )
}

fn pane(
    launch: &debugger::Pane,
    focus: &debugger::Focus,
    at: usize,
    current: bool,
) -> Result<PaneView> {
    check_focus(launch, focus)?;

    Ok(PaneView {
        at,
        current,
        status: status_bar(launch, &Fragment::Status.id(at))?,
        dockbar: dockbar(launch, &Fragment::Dockbar.id(at))?,

        source: source(launch, focus, &Fragment::Source.id(at))?,
        groups: groups(launch, focus, &Fragment::Groups.id(at))?,
        paths: paths(launch, focus, &Fragment::Paths.id(at))?,

        items: items(launch, focus, at)?,
        side: side(launch, focus, at)?,
        dock: dock(launch, focus, at)?,
    })
}

fn check_focus(launch: &debugger::Pane, focus: &debugger::Focus) -> Result<()> {
    let total = launch.total();

    if focus.group >= total {
        return refuse(format!(
            "selected group {} is outside the {total} that exist",
            focus.group
        ));
    }

    if !launch.showing(focus).is_empty() && focus.path >= launch.showing(focus).len() {
        return refuse(format!(
            "path {} is focused but the launch only has {}",
            focus.path,
            launch.showing(focus).len()
        ));
    }

    let deepest = launch
        .paths
        .iter()
        .map(|path| path.frames.len())
        .max()
        .unwrap_or(0);

    if focus.frame > 0 && focus.frame >= deepest {
        return refuse(format!(
            "frame {} is selected but the deepest stack holds {deepest}",
            focus.frame
        ));
    }

    if !launch.diagnostics.is_empty() && focus.finding >= launch.diagnostics.len() {
        return refuse(format!(
            "diagnostic {} is selected but the launch only has {}",
            focus.finding,
            launch.diagnostics.len()
        ));
    }

    Ok(())
}

fn tabs(view: &debugger::Page, id: &str) -> Result<String> {
    paint(
        "tabs",
        &TabsView {
            id: id.to_string(),
            tabs: view
                .tabs
                .iter()
                .enumerate()
                .map(|(at, launch)| TabRow {
                    at,
                    name: launch.name.to_string(),
                    state: launch.state,
                    current: at == view.selected,
                })
                .collect(),
        },
    )
}

fn groups(launch: &debugger::Pane, focus: &debugger::Focus, id: &str) -> Result<String> {
    let total = launch.total();

    if launch.dispatched > total {
        return refuse(format!(
            "{} groups dispatched but the launch only has {total}",
            launch.dispatched
        ));
    }

    if launch.mix.len() as u64 != total {
        return refuse(format!(
            "expected a mix for each of {total} groups but got {}",
            launch.mix.len()
        ));
    }

    for (group, shares) in launch.mix.iter().enumerate() {
        let group = group as u64;

        if shares.is_empty() {
            let settled = group < launch.dispatched
                && !launch.running.contains(&group)
                && !launch.faulted.contains(&group);

            if settled {
                return refuse(format!(
                    "group {group} has finished but has an empty mix, every group that has run is somewhere in the program"
                ));
            }

            continue;
        }

        for share in shares {
            if share.path >= COLOURS.len() {
                return refuse(format!(
                    "group {group} names path {} but there are only {} colours",
                    share.path,
                    COLOURS.len()
                ));
            }

            if share.part.is_nan() || share.part <= 0.0 {
                return refuse(format!(
                    "group {group} gives path {} a share of {}, shares have to be above zero",
                    share.path, share.part
                ));
            }
        }
    }

    for group in &launch.running {
        if *group >= launch.dispatched {
            return refuse(format!(
                "group {group} is running but only {} have been dispatched",
                launch.dispatched
            ));
        }
    }

    for group in &launch.faulted {
        if *group >= total {
            return refuse(format!(
                "faulted group {group} is outside the {total} that exist"
            ));
        }
    }

    let shape = lattice_shape(launch.counts, true)?;
    let done = launch.dispatched - launch.running.len() as u64;
    let planes = lattice_planes(launch.counts, &shape, |at| {
        group_cell(launch, focus, at, shape.block)
    });

    paint(
        "lattice",
        &LatticeView {
            id: id.to_string(),
            kind: "groups",
            title: "Work groups",
            note: format!("{}, {done} done", extent_label(launch.counts)),
            columns: shape.columns,
            rows: shape.rows,
            dims: dims(launch.counts),
            labelled: false,
            stacked: launch.counts[2] > 1,
            block: shape.block,
            stride: launch.counts[0],
            planes,
        },
    )
}

fn group_cell(
    launch: &debugger::Pane,
    focus: &debugger::Focus,
    at: [u64; 3],
    block: u64,
) -> LatticeCell {
    let counts = launch.counts;

    let members: Vec<u64> = (0..block)
        .flat_map(|dy| (0..block).map(move |dx| (dx, dy)))
        .filter_map(|(dx, dy)| {
            let (x, y) = (at[0] + dx, at[1] + dy);

            (x < counts[0] && y < counts[1]).then(|| (at[2] * counts[1] + y) * counts[0] + x)
        })
        .collect();

    let alive = members
        .iter()
        .map(|index| group_life(launch, *index))
        .min_by_key(|life| match life {
            inspect::Life::Running => 0,
            inspect::Life::Parked => 1,
            inspect::Life::Done => 2,
            inspect::Life::Pending => 3,
        })
        .unwrap_or(inspect::Life::Pending);

    let mut mix: Vec<debugger::Tint> = Vec::new();
    for index in members
        .iter()
        .filter(|index| group_life(launch, **index) != inspect::Life::Pending)
    {
        for share in &launch.mix[*index as usize] {
            match mix.iter_mut().find(|held| held.path == share.path) {
                Some(held) => held.part += share.part,
                None => mix.push(*share),
            }
        }
    }

    let leader = members[0];

    LatticeCell {
        label: leader.to_string(),
        fill: match mix.is_empty() {
            true => String::new(),
            false => cell_fill(&mix),
        },
        life: alive,
        marked: members.iter().any(|index| launch.faulted.contains(index)),
        selected: members.contains(&focus.group),
        tip: match block {
            1 => format!("group {}, {}, {}, index {leader}", at[0], at[1], at[2]),
            _ => format!(
                "{} groups from {}, {}, {}",
                members.len(),
                at[0],
                at[1],
                at[2]
            ),
        },
        pick: format!("group:{leader}"),
    }
}

fn group_life(launch: &debugger::Pane, index: u64) -> inspect::Life {
    if launch.running.contains(&index) {
        return inspect::Life::Running;
    }

    match index < launch.dispatched {
        true => inspect::Life::Done,
        false => inspect::Life::Pending,
    }
}

fn lattice_shape(extent: [u64; 3], aggregate: bool) -> Result<LatticeShape> {
    for (axis, span) in extent.iter().enumerate() {
        if *span == 0 {
            return refuse(format!(
                "axis {axis} is zero, every axis needs at least one"
            ));
        }
    }

    if extent[2] > MAX_PLANES {
        return refuse(format!(
            "{} planes is more than the {MAX_PLANES} this can draw",
            extent[2]
        ));
    }

    let block = match (aggregate, extent[1]) {
        (false, _) => 1,
        (true, 1) => extent[0].div_ceil(MAX_CELLS).max(1),
        (true, _) => extent[0].max(extent[1]).div_ceil(MAX_SIDE).max(1),
    };

    Ok(LatticeShape {
        columns: extent[0].div_ceil(block),
        rows: extent[1].div_ceil(block),
        block,
    })
}

fn lattice_planes(
    extent: [u64; 3],
    shape: &LatticeShape,
    mut make: impl FnMut([u64; 3]) -> LatticeCell,
) -> Vec<LatticePlane> {
    (0..extent[2])
        .map(|depth| LatticePlane {
            depth,
            rows: (0..shape.rows)
                .map(|row| {
                    (0..shape.columns)
                        .filter_map(|column| {
                            let x = column * shape.block;

                            (x < extent[0]).then(|| make([x, row * shape.block, depth]))
                        })
                        .collect()
                })
                .collect(),
        })
        .collect()
}

fn cell_fill(mix: &[debugger::Tint]) -> String {
    if mix.len() == 1 {
        return format!("background: {}", path_colour(mix[0].path));
    }

    let total: f32 = mix.iter().map(|share| share.part).sum();
    let mut at = 0.0;

    let stops: Vec<String> = mix
        .iter()
        .map(|share| {
            let from = at / total * 100.0;
            at += share.part;

            format!(
                "{} {from:.2}% {:.2}%",
                path_colour(share.path),
                at / total * 100.0
            )
        })
        .collect();

    format!("background: linear-cell_fill(180deg, {})", stops.join(", "))
}

fn path_colour(path: usize) -> &'static str {
    COLOURS[path % COLOURS.len()]
}

fn items(launch: &debugger::Pane, focus: &debugger::Focus, at: usize) -> Result<String> {
    let id = Fragment::Items.id(at);

    let held = launch
        .resident()
        .find(|group| group.index == focus.group)
        .map(|group| items_lattice(launch, group, focus, ""))
        .transpose()?;

    let body = match held {
        Some(body) => body,
        None => empty("", "Work items", absent_group(launch, focus.group))?,
    };

    paint("items", &ItemsView { id, body })
}

fn items_lattice(
    launch: &debugger::Pane,
    group: &debugger::GroupCells,
    focus: &debugger::Focus,
    id: &str,
) -> Result<String> {
    let shape = lattice_shape(launch.local, false)?;
    let flat = launch.local[0] * launch.local[1];
    let total = launch.lanes();

    if total > MAX_LANES {
        return refuse(format!(
            "{total} lanes is more than the device allows, the limit is {MAX_LANES}"
        ));
    }

    if group.lanes.len() as u64 != total {
        return refuse(format!(
            "expected {total} lanes to fill work group {} but got {}",
            group.index,
            group.lanes.len()
        ));
    }

    if let Some(lane) = focus.lane
        && lane >= total
    {
        return refuse(format!(
            "lane {lane} is selected but work group {} only holds {total}",
            group.index
        ));
    }

    let cell = |lane: u64| {
        let held = group.lanes[lane as usize];
        let key = format!("{}/{lane}", group.index);

        LatticeCell {
            label: lane.to_string(),
            life: held.life,
            fill: match (held.life, held.path) {
                (inspect::Life::Pending, _) | (_, None) => String::new(),
                (_, Some(path)) => format!("background: {}", path_colour(path)),
            },
            marked: held.marked,
            selected: group.index == focus.group && focus.lane == Some(lane),
            tip: match held.path {
                Some(path) => format!("lane {lane}, P{path}"),
                None => format!("lane {lane}, no path"),
            },
            pick: match held.path {
                Some(path) => format!("path:{path} lane:{key}"),
                None => format!("lane:{key}"),
            },
        }
    };

    let planes = lattice_planes(launch.local, &shape, |at| {
        cell(at[2] * flat + at[1] * launch.local[0] + at[0])
    });

    let done = group
        .lanes
        .iter()
        .filter(|lane| lane.life == inspect::Life::Done)
        .count();

    paint(
        "lattice",
        &LatticeView {
            id: id.to_string(),
            kind: "items",
            title: "Work items",
            note: format!("{}, {done} done", extent_label(launch.local)),
            columns: shape.columns,
            rows: shape.rows,
            dims: dims(launch.local),
            labelled: total <= 64,
            stacked: launch.local[2] > 1,
            block: shape.block,
            stride: launch.local[0],
            planes,
        },
    )
}

fn absent_group(launch: &debugger::Pane, group: u64) -> &'static str {
    match group < launch.dispatched {
        true => "This work group has run, its work items are no longer held.",
        false => "This work group has not started.",
    }
}

fn source(launch: &debugger::Pane, focus: &debugger::Focus, id: &str) -> Result<String> {
    if launch.lines.is_empty() {
        return refuse("the source has no lines, there is nothing to show");
    }

    for pair in launch.lines.windows(2) {
        if pair[1].number <= pair[0].number {
            return refuse(format!(
                "line {} follows line {}, lines have to climb",
                pair[1].number, pair[0].number
            ));
        }
    }

    let rows = launch
        .lines
        .iter()
        .zip(highlighted_source(&launch.lines).iter().cloned())
        .enumerate()
        .map(|(at, (line, body))| SourceRow {
            number: line.number,
            stop: line.stop,
            body,
            mark: source_mark(line.mark),
            gap: match at {
                0 => 0,
                _ => line.number - launch.lines[at - 1].number - 1,
            },
        })
        .collect();

    paint(
        "source",
        &SourceView {
            id: id.to_string(),
            file: launch.file.to_string(),
            rows,
            controls: control_bar(launch)?,
        },
    )
}

fn control_bar(launch: &debugger::Pane) -> Result<ControlBarView> {
    if launch.commands.is_empty() {
        return refuse("a control bar with no commands, there would be nothing to press");
    }

    for (at, command) in launch.commands.iter().enumerate() {
        if launch.commands[..at]
            .iter()
            .any(|earlier| earlier.control == command.control)
        {
            return refuse(format!("{} appears twice", command.control.label()));
        }
    }

    Ok(ControlBarView {
        state: launch.state,
        state_label: launch.state.label(),
        commands: launch
            .commands
            .iter()
            .map(|command| ControlButton {
                control: command.control,
                label: command.control.label(),
                icon: control_icon(command.control),
                enabled: command.enabled,
                send: format!("do={}", control_name(command.control)),
            })
            .collect(),
    })
}

fn source_mark(mark: Option<debugger::SourceMark>) -> &'static str {
    match mark {
        Some(debugger::SourceMark::Current) => "current",
        Some(debugger::SourceMark::Fault) => "fault",
        None => "",
    }
}

fn control_name(control: debugger::Control) -> &'static str {
    match control {
        debugger::Control::Resume => "resume",
        debugger::Control::Step => "step",
    }
}

fn control_icon(control: debugger::Control) -> &'static str {
    match control {
        debugger::Control::Resume => RESUME,
        debugger::Control::Step => STEP,
    }
}

fn highlighted_source(lines: &[debugger::SourceLine]) -> Arc<Vec<String>> {
    let mut hasher = DefaultHasher::new();

    for line in lines {
        line.text.hash(&mut hasher);
    }

    let key = hasher.finish();
    let cache = HIGHLIGHTS.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(held) = cache.lock().expect("highlights").get(&key) {
        return Arc::clone(held);
    }

    let made = Arc::new(highlight_lines(lines));
    let mut cache = cache.lock().expect("highlights");

    if cache.len() >= MAX_HIGHLIGHTS {
        cache.clear();
    }

    cache.insert(key, Arc::clone(&made));

    made
}

fn highlight_lines(lines: &[debugger::SourceLine]) -> Vec<String> {
    let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);

    let Some(syntax) = syntaxes.find_syntax_by_extension("c") else {
        return lines.iter().map(|line| escape(&line.text)).collect();
    };

    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();

    lines
        .iter()
        .map(|line| {
            let text = format!("{}\n", line.text);
            let carried = open_spans(&stack);

            let Ok(operations) = state.parse_line(&text, syntaxes) else {
                return escape(&line.text);
            };

            let drawn = line_tokens_to_classed_spans(
                &text,
                &operations[..],
                ClassStyle::Spaced,
                &mut stack,
            );

            match drawn {
                Ok((body, _)) => {
                    let body = body.replace('\n', "");
                    let closing = "</span>".repeat(stack.len());

                    format!("{carried}{body}{closing}")
                }
                Err(_) => escape(&line.text),
            }
        })
        .collect()
}

fn open_spans(stack: &ScopeStack) -> String {
    stack
        .as_slice()
        .iter()
        .map(|scope| {
            format!(
                "<span class=\"{}\">",
                scope.build_string().replace('.', " ")
            )
        })
        .collect()
}

fn paths(launch: &debugger::Pane, focus: &debugger::Focus, id: &str) -> Result<String> {
    if launch.showing(focus).is_empty() {
        return empty(id, "Execution paths", absent_group(launch, focus.group));
    }

    for path in launch.showing(focus) {
        if path.items == 0 {
            return refuse(format!("path {} holds no work items", path.name));
        }
    }

    let total: u64 = launch.showing(focus).iter().map(|path| path.items).sum();
    let tracks = launch
        .paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let at = path.at.to_string();

            let (label, tip) = match &path.at.text {
                Some(text) => (text.clone(), format!("{at}  {text}")),
                None => (at.clone(), at),
            };

            PathRow {
                name: path.name.to_string(),
                colour: path.colour.to_string(),
                label,
                tip,
                items: thousands(path.items),
                share: format!("{:.4}%", path.items as f64 / total as f64 * 100.0),
                current: index == focus.path,
                pick: format!("path:{index}"),
            }
        })
        .collect();

    paint(
        "paths",
        &PathsView {
            id: id.to_string(),
            note: format!(
                "{} over {}",
                plural(launch.showing(focus).len() as u64, "path"),
                thousands(total)
            ),
            tracks,
        },
    )
}

fn side(launch: &debugger::Pane, focus: &debugger::Focus, at: usize) -> Result<String> {
    let id = Fragment::Side.id(at);

    let Some(path) = launch.showing(focus).get(focus.path) else {
        let missing = absent_group(launch, focus.group);

        return paint(
            "side",
            &SideView {
                id,
                values: empty("", "Values", missing)?,
                stack: empty("", "Call stack", missing)?,
            },
        );
    };

    paint(
        "side",
        &SideView {
            id,
            values: values(path, "")?,
            stack: call_stack(path, focus, "")?,
        },
    )
}

fn values(path: &debugger::PathRow, id: &str) -> Result<String> {
    let values = path
        .values
        .iter()
        .map(|value| {
            let (shown, tone) = match &value.shown {
                Some(shown) => (shown.clone(), ""),
                None => ("not available here".to_string(), "gone"),
            };

            ValueRow {
                name: value.name.clone(),
                at: value.at.to_string(),
                shown,
                tone,
                type_name: value.type_name.clone(),
                kind: value.kind,
            }
        })
        .collect();

    paint(
        "values",
        &ValuesView { id: id.to_string(), scope: path.name.to_string(), values },
    )
}

fn call_stack(path: &debugger::PathRow, focus: &debugger::Focus, id: &str) -> Result<String> {
    let frames = path
        .frames
        .iter()
        .enumerate()
        .map(|(depth, frame)| {
            let (name, tone) = match frame.name.is_empty() {
                true => ("unnamed".to_string(), "gone"),
                false => (frame.name.clone(), ""),
            };

            FrameRow {
                name,
                tone,
                at: site_label(frame.at.as_ref(), ""),
                text: site_text(frame.at.as_ref()),
                selected: depth == focus.frame,
                depth,
            }
        })
        .collect();

    paint(
        "stack",
        &CallStackView {
            id: id.to_string(),
            note: plural(path.frames.len() as u64, "frame"),
            frames,
        },
    )
}

fn dock(launch: &debugger::Pane, focus: &debugger::Focus, at: usize) -> Result<String> {
    let id = Fragment::Dock.id(at);

    let held = launch
        .diagnostics
        .get(focus.finding)
        .map(|finding| details(finding, ""))
        .transpose()?;

    paint(
        "dock",
        &DockView {
            id,
            findings: findings(launch, focus, &format!("findings-{at}"))?,
            details: held.unwrap_or_default(),
        },
    )
}

fn findings(launch: &debugger::Pane, focus: &debugger::Focus, id: &str) -> Result<String> {
    paint(
        "findings",
        &FindingsView {
            id: id.to_string(),
            findings: launch
                .diagnostics
                .iter()
                .enumerate()
                .map(|(index, finding)| FindingRow {
                    severity: finding.severity,
                    headline: finding.headline.clone(),
                    detail: finding.detail.clone(),
                    at: site_label(finding.at.as_ref(), "no line information"),
                    selected: index == focus.finding,
                    pick: format!("finding:{index}"),
                })
                .collect(),
        },
    )
}

fn site_label(at: Option<&inspect::Site>, absent: &str) -> String {
    match at {
        Some(site) => site.to_string(),
        None => absent.to_string(),
    }
}

fn site_text(at: Option<&inspect::Site>) -> String {
    match at.and_then(|site| site.text.as_ref()) {
        Some(text) => text.clone(),
        None => String::new(),
    }
}

fn worst_severity(findings: &[inspect::Diagnostic]) -> &'static str {
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == inspect::Severity::Error)
        .count();

    match (findings.is_empty(), errors) {
        (true, _) => "ok",
        (_, 0) => "warn",
        _ => "err",
    }
}

fn details(finding: &inspect::Diagnostic, id: &str) -> Result<String> {
    let rows = finding
        .rows
        .iter()
        .map(|row| DetailRow {
            label: row.label.clone(),
            value: match row.value.is_empty() {
                true => "unknown".to_string(),
                false => row.value.clone(),
            },
        })
        .collect();

    paint("details", &DetailsView { id, rows })
}

fn dockbar(launch: &debugger::Pane, id: &str) -> Result<String> {
    paint(
        "dockbar",
        &DockbarView {
            id: id.to_string(),
            problems: thousands(launch.diagnostics.len() as u64),
            severity: worst_severity(&launch.diagnostics),
        },
    )
}

fn status_bar(launch: &debugger::Pane, id: &str) -> Result<String> {
    let counts = launch.counts;
    let local = launch.local;

    paint(
        "status",
        &StatusBarView {
            id: id.to_string(),
            state: launch.state,
            state_label: launch.state.label(),
            global: extent_label([
                counts[0] * local[0],
                counts[1] * local[1],
                counts[2] * local[2],
            ]),
            local: extent_label(local),
            progress: format!(
                "{} of {}",
                thousands(launch.dispatched),
                thousands(launch.total())
            ),
            problems: thousands(launch.diagnostics.len() as u64),
            severity: worst_severity(&launch.diagnostics),
        },
    )
}

pub fn missing(what: &str) -> Error {
    Error(format!("{what} is not something this page can show"))
}

pub fn unavailable(id: &str, title: &str, error: &Error) -> String {
    let context = UnavailableView { id, title, reason: error.to_string() };

    paint("unavailable", &context).unwrap_or_else(|_| {
        REFUSED
            .replace("{title}", title)
            .replace("{reason}", &context.reason)
    })
}

fn empty(id: &str, title: &str, message: &str) -> Result<String> {
    paint("empty", &EmptyView { id, title, message })
}

fn paint<T: Serialize>(name: &str, context: &T) -> Result<String> {
    let environment = ENVIRONMENT
        .get_or_init(|| {
            let mut environment = Environment::new();
            environment.set_auto_escape_callback(|_| AutoEscape::Html);
            environment.add_filter("flag", flag);

            for (named, body) in PAGES {
                environment.add_template(named, body)?;
            }

            Ok(environment)
        })
        .as_ref()
        .map_err(Clone::clone)?;

    Ok(environment.get_template(name)?.render(context)?)
}

fn flag(value: bool) -> &'static str {
    match value {
        true => "true",
        false => "false",
    }
}

fn refuse<T>(reason: impl fmt::Display) -> Result<T> {
    Err(Error(reason.to_string()))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();

    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }

    out
}

fn plural(count: u64, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

fn extent_label(value: [u64; 3]) -> String {
    format!("{} x {} x {}", value[0], value[1], value[2])
}

fn dims(extent: [u64; 3]) -> u8 {
    match extent {
        [_, _, z] if z > 1 => 3,
        [_, y, _] if y > 1 => 2,
        _ => 1,
    }
}
