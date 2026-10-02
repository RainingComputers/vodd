use crate::hypermedia;
use crate::hypermedia::Html;
use crate::inspect;
use crate::state;
use maud::Markup;
use maud::PreEscaped;
use maud::html;
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

pub const STYLE: &str = concat!(
    include_str!("templates/fonts.css"),
    include_str!("templates/style.css")
);
const RESUME: &str = include_str!("templates/icons/resume.svg");
const STEP: &str = include_str!("templates/icons/step.svg");
const STOP: &str = include_str!("templates/icons/stop.svg");
const UNPLUGGED: &str = include_str!("templates/icons/unplugged.svg");
const SCRIPT: &str = include_str!("templates/debugger.js");

const MAX_SIDE: u64 = 32;
const MAX_CELLS: u64 = MAX_SIDE * MAX_SIDE;
const MAX_PLANES: u64 = 64;
const MAX_LANES: u64 = 256;
const MAX_HIGHLIGHTS: usize = 32;
static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static HIGHLIGHTS: OnceLock<Mutex<HashMap<u64, Arc<Vec<String>>>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

struct LatticeCell {
    label: String,
    life: inspect::Life,
    fill: String,
    marked: bool,
    selected: bool,
    tip: String,
    pick: String,
}

struct LatticePlane {
    depth: u64,
    rows: Vec<Vec<LatticeCell>>,
}

struct LatticeShape {
    columns: u64,
    rows: u64,
    block: u64,
}

type Result<T> = core::result::Result<T, Error>;

pub fn draw(view: &state::Page) -> Html {
    match tree(view) {
        Ok(drawn) => drawn,
        Err(error) => shared("page", Err(error)),
    }
}

pub fn shell(view: &state::Page) -> String {
    let body = hypermedia::render(&draw(view));

    html! {
        (PreEscaped("<!doctype html>"))
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "vodd" }
                link rel="stylesheet" href="/style.css";
            }
            body {
                p class="link mono" role="status" {
                    svg width="13" height="13" viewBox="0 0 16 16" fill="none"
                        stroke="currentColor" stroke-width="1.7" stroke-linecap="round"
                        stroke-linejoin="round" aria-hidden="true" {
                        (PreEscaped(UNPLUGGED))
                    }
                    " Debugger disconnected"
                }
                (PreEscaped(body))
                (PreEscaped(hypermedia::SCRIPTS))
                script { (PreEscaped(SCRIPT)) }
            }
        }
    }
    .into_string()
}

pub fn missing(what: &str) -> Error {
    Error(format!("{what} is not something this page can show"))
}

pub fn unavailable(id: &str, title: &str, error: &Error) -> String {
    html! {
        figure class="widget unavailable" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " " (title) " " }
                span class="note mono" { " unavailable " }
            }
            p class="reason" { (error) }
        }
    }
    .into_string()
}

fn tree(view: &state::Page) -> Result<Html> {
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

    let strip = shared("tabs", tabs(view, "tabs"));
    let panes = view
        .tabs
        .iter()
        .enumerate()
        .map(|(at, launch)| pane(launch, &view.focus[at], at, at == view.selected))
        .collect::<Result<Vec<Html>>>()?;

    Ok(hypermedia::tag(
        "main",
        &[("class", "page"), ("id", "page")],
        std::iter::once(strip).chain(panes).collect(),
    ))
}

fn leaf(name: &str, at: usize, drawn: Result<Markup>) -> Html {
    tagged(ident(name, at), name, drawn)
}

fn shared(name: &str, drawn: Result<Markup>) -> Html {
    tagged(name.to_string(), name, drawn)
}

fn tagged(id: String, name: &str, drawn: Result<Markup>) -> Html {
    let body = match drawn {
        Ok(markup) => markup.into_string(),
        Err(error) => unavailable(&id, name, &error),
    };

    Html::Leaf { id, body }
}

fn ident(name: &str, at: usize) -> String {
    format!("{name}-{at}")
}

fn pane(launch: &state::Pane, focus: &state::Focus, at: usize, current: bool) -> Result<Html> {
    check_focus(launch, focus)?;

    let index = at.to_string();
    let show = format!("tab:{at}");

    let mut attrs = vec![
        ("class", "pane"),
        ("role", "tabpanel"),
        ("data-launch", index.as_str()),
        ("data-show", show.as_str()),
    ];

    if !current {
        attrs.push(("hidden", ""));
    }

    let rail = hypermedia::tag(
        "div",
        &[("class", "col rail")],
        vec![
            hold(
                "hold",
                leaf("groups", at, groups(launch, focus, &ident("groups", at))),
            ),
            leaf("items", at, items(launch, focus, at)),
            hold(
                "hold grow",
                leaf("paths", at, paths(launch, focus, &ident("paths", at))),
            ),
        ],
    );

    let workspace = hypermedia::tag(
        "div",
        &[("class", "workspace")],
        vec![
            hypermedia::tag(
                "div",
                &[("class", "col")],
                vec![hold(
                    "hold grow",
                    leaf("source", at, source(launch, at, &ident("source", at))),
                )],
            ),
            grip("wide", "--rail-right", "page", "-1", "vertical"),
            leaf("side", at, side(launch, focus, at)),
        ],
    );

    let dock = hypermedia::tag(
        "div",
        &[("class", "dock")],
        vec![
            leaf("dockbar", at, dockbar(launch, &ident("dockbar", at))),
            leaf("dock", at, dock(launch, focus, at)),
        ],
    );

    Ok(hypermedia::tag(
        "section",
        &attrs,
        vec![
            rail,
            grip("wide tallest", "--rail-left", "page", "1", "vertical"),
            workspace,
            grip("tall", "--dock", "pane", "-1", "horizontal"),
            dock,
            leaf("status", at, status_bar(launch, &ident("status", at))),
        ],
    ))
}

fn hold(class: &str, kid: Html) -> Html {
    hypermedia::tag("div", &[("class", class)], vec![kid])
}

fn grip(class: &str, drive: &str, scope: &str, sign: &str, orientation: &str) -> Html {
    let class = format!("grip {class}");

    hypermedia::tag(
        "div",
        &[
            ("class", class.as_str()),
            ("data-drive", drive),
            ("data-scope", scope),
            ("data-sign", sign),
            ("role", "separator"),
            ("aria-orientation", orientation),
        ],
        Vec::new(),
    )
}

fn check_focus(launch: &state::Pane, focus: &state::Focus) -> Result<()> {
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

fn tabs(view: &state::Page, id: &str) -> Result<Markup> {
    Ok(html! {
        nav class="tabs" role="tablist" id=(id) {
            @for (at, launch) in view.tabs.iter().enumerate() {
                button class="tab across" role="tab"
                       aria-selected=(flag(at == view.selected))
                       data-pick=(format!("tab:{at}"))
                       data-mark=(format!("tab:{at}")) {
                    span class=(format!("dot {}", status_name(launch.status))) {}
                    " " (launch.name)
                }
            }
        }
    })
}

fn groups(launch: &state::Pane, focus: &state::Focus, id: &str) -> Result<Markup> {
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
                && launch.running != Some(group)
                && !launch.faulted.contains(&group);

            if settled {
                return refuse(format!(
                    "group {group} has finished but has an empty mix, every group that has run is somewhere in the program"
                ));
            }

            continue;
        }

        for share in shares {
            if share.path >= state::COLOURS.len() {
                return refuse(format!(
                    "group {group} names path {} but there are only {} colours",
                    share.path,
                    state::COLOURS.len()
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

    if let Some(group) = launch.running
        && group >= launch.dispatched
    {
        return refuse(format!(
            "group {group} is running but only {} have been dispatched",
            launch.dispatched
        ));
    }

    for group in &launch.faulted {
        if *group >= total {
            return refuse(format!(
                "faulted group {group} is outside the {total} that exist"
            ));
        }
    }

    let shape = lattice_shape(launch.counts, true)?;
    let done = launch.dispatched - u64::from(launch.running.is_some());
    let planes = lattice_planes(launch.counts, &shape, |at| {
        group_cell(launch, focus, at, shape.block)
    });

    Ok(lattice(
        id,
        "groups",
        "Work groups",
        &format!("{}, {done} done", extent_label(launch.counts)),
        launch.counts,
        &shape,
        false,
        launch.counts[0],
        &planes,
    ))
}

fn group_cell(launch: &state::Pane, focus: &state::Focus, at: [u64; 3], block: u64) -> LatticeCell {
    let counts = launch.counts;

    let members = || {
        (0..block)
            .flat_map(|dy| (0..block).map(move |dx| (dx, dy)))
            .filter_map(move |(dx, dy)| {
                let (x, y) = (at[0] + dx, at[1] + dy);

                (x < counts[0] && y < counts[1]).then(|| (at[2] * counts[1] + y) * counts[0] + x)
            })
    };

    let alive = members()
        .map(|index| group_life(launch, index))
        .min_by_key(|life| match life {
            inspect::Life::Running => 0,
            inspect::Life::Parked => 1,
            inspect::Life::Done => 2,
            inspect::Life::Pending => 3,
        })
        .unwrap_or(inspect::Life::Pending);

    let mix = members()
        .filter(|index| group_life(launch, *index) != inspect::Life::Pending)
        .flat_map(|index| &launch.mix[index as usize])
        .fold(Vec::<state::Tint>::new(), |mut mix, share| {
            match mix.iter_mut().find(|held| held.path == share.path) {
                Some(held) => held.part += share.part,
                None => mix.push(*share),
            }

            mix
        });

    let leader = members().next().unwrap_or(0);

    LatticeCell {
        label: leader.to_string(),
        fill: match mix.is_empty() {
            true => String::new(),
            false => cell_fill(&mix),
        },
        life: alive,
        marked: members().any(|index| launch.faulted.contains(&index)),
        selected: members().any(|index| index == focus.group),
        tip: match block {
            1 => format!("group {}, {}, {}, index {leader}", at[0], at[1], at[2]),
            _ => format!(
                "{} groups from {}, {}, {}",
                members().count(),
                at[0],
                at[1],
                at[2]
            ),
        },
        pick: format!("group:{leader}"),
    }
}

fn group_life(launch: &state::Pane, index: u64) -> inspect::Life {
    if launch.running == Some(index) {
        return inspect::Life::Running;
    }

    match index < launch.dispatched {
        true => inspect::Life::Done,
        false => inspect::Life::Pending,
    }
}

#[allow(clippy::too_many_arguments)]
fn items(launch: &state::Pane, focus: &state::Focus, at: usize) -> Result<Markup> {
    let id = ident("items", at);

    let held = launch
        .resident()
        .find(|group| group.index == focus.group)
        .map(|group| items_lattice(launch, group, focus, ""))
        .transpose()?;

    let body = match held {
        Some(body) => body,
        None => empty("", "Work items", absent_group(launch, focus.group)),
    };

    Ok(html! {
        div class="swaps" id=(id) {
            div class="hold" { (body) }
        }
    })
}

fn items_lattice(
    launch: &state::Pane,
    group: &state::GroupCells,
    focus: &state::Focus,
    id: &str,
) -> Result<Markup> {
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

    Ok(lattice(
        id,
        "items",
        "Work items",
        &format!("{}, {done} done", extent_label(launch.local)),
        launch.local,
        &shape,
        total <= 64,
        launch.local[0],
        &planes,
    ))
}

fn lattice(
    id: &str,
    kind: &str,
    title: &str,
    note: &str,
    extent: [u64; 3],
    shape: &LatticeShape,
    labelled: bool,
    stride: u64,
    planes: &[LatticePlane],
) -> Markup {
    let stacked = extent[2] > 1;

    html! {
        figure class=(format!("widget lattice {kind} d{}", dims(extent)))
               style=(format!("--x: {}; --y: {}; --planes: {}", shape.columns, shape.rows, planes.len()))
               data-block=(shape.block)
               data-stride=(stride)
               id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " " (title) " " }
                span class="note mono" { " " (note) " " }
            }
            div class="planes" {
                @for plane in planes {
                    div class="plane" {
                        @if stacked {
                            span class="depth mono" { " z = " (plane.depth) " " }
                        }
                        div class="rows" {
                            @for row in &plane.rows {
                                div class="row" {
                                    @for cell in row {
                                        button class=(cell_class(cell))
                                               style=(cell.fill)
                                               title=(cell.tip)
                                               data-pick=(cell.pick) {
                                            @if labelled {
                                                b class="mono" { (cell.label) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn cell_class(cell: &LatticeCell) -> String {
    let mut out = format!("cell {}", life_name(cell.life));

    if cell.marked {
        out.push_str(" marked");
    }

    if cell.selected {
        out.push_str(" on");
    }

    out
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

fn cell_fill(mix: &[state::Tint]) -> String {
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

    format!("background: linear-gradient(180deg, {})", stops.join(", "))
}

fn path_colour(path: usize) -> &'static str {
    state::COLOURS[path % state::COLOURS.len()]
}

fn source(launch: &state::Pane, at: usize, id: &str) -> Result<Markup> {
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

    let controls = control_bar(launch, at)?;
    let bodies = highlighted_source(&launch.lines);

    Ok(html! {
        figure class="widget source" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " Source " }
                span class="note mono" { " " (launch.file) " " }
            }
            (controls)
            div class="code mono" {
                @for (row, line) in launch.lines.iter().enumerate() {
                    @let gap = match row {
                        0 => 0,
                        _ => line.number - launch.lines[row - 1].number - 1,
                    };
                    @if gap > 0 {
                        div class="elided" { (gap) " lines not shown" }
                    }
                    div class=(line_class(line)) {
                        button data-post="/breakpoint"
                               data-send=(format!("launch={at}&line={}", line.number))
                               title=(format!("Toggle a breakpoint on line {}", line.number)) {
                            (line.number)
                        }
                        code { (PreEscaped(bodies[row].clone())) }
                    }
                }
            }
        }
    })
}

fn line_class(line: &state::SourceLine) -> String {
    let mut out = format!("ln {}", source_mark(line.mark));

    if line.stop {
        out.push_str(" stop");
    }

    out
}

fn control_bar(launch: &state::Pane, at: usize) -> Result<Markup> {
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

    Ok(html! {
        div class="controls across wrap" {
            div class="transport" {
                @for command in &launch.commands {
                    button class=(format!("key {}", control_name(command.control)))
                           title=(command.control.label())
                           aria-label=(command.control.label())
                           data-post="/control"
                           data-send=(format!("launch={at}&do={}", control_name(command.control)))
                           disabled[!command.enabled] {
                        svg viewBox="0 0 16 16" fill="none" stroke="currentColor"
                            stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"
                            aria-hidden="true" {
                            (PreEscaped(control_icon(command.control)))
                        }
                    }
                }
            }
            span class=(format!("stance mono {}", status_name(launch.status))) {
                " " (launch.status.label()) " "
            }
        }
    })
}

fn highlighted_source(lines: &[state::SourceLine]) -> Arc<Vec<String>> {
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

fn highlight_lines(lines: &[state::SourceLine]) -> Vec<String> {
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

fn paths(launch: &state::Pane, focus: &state::Focus, id: &str) -> Result<Markup> {
    if launch.showing(focus).is_empty() {
        return Ok(empty(
            id,
            "Execution paths",
            absent_group(launch, focus.group),
        ));
    }

    for path in launch.showing(focus) {
        if path.items == 0 {
            return refuse(format!("path {} holds no work items", path.name));
        }
    }

    let total: u64 = launch.showing(focus).iter().map(|path| path.items).sum();
    let note = format!(
        "{} over {}",
        plural(launch.showing(focus).len() as u64, "path"),
        thousands(total)
    );

    Ok(html! {
        figure class="widget" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " Execution paths " }
                span class="note mono" { " " (note) " " }
            }
            div class="body" {
                div class="bar" {
                    @for path in &launch.paths {
                        span style=(format!(
                            "background: {}; width: {:.4}%",
                            path.colour,
                            path.items as f64 / total as f64 * 100.0
                        )) {}
                    }
                }
                ul class="tracks" {
                    @for (index, path) in launch.paths.iter().enumerate() {
                        @let at = path.at.to_string();
                        @let (label, tip) = match &path.at.text {
                            Some(text) => (text.clone(), format!("{at}  {text}")),
                            None => (at.clone(), at.clone()),
                        };
                        li class="across" aria-current=(flag(index == focus.path))
                           data-pick=(format!("path:{index}")) {
                            span class="chip" style=(format!("background: {}", path.colour)) {}
                            b { " " (path.name) " " }
                            span class="grew mono clip" title=(tip) { " " (label) " " }
                            span class="tally mono" { " " (thousands(path.items)) " " }
                        }
                    }
                }
            }
        }
    })
}

fn side(launch: &state::Pane, focus: &state::Focus, at: usize) -> Result<Markup> {
    let id = ident("side", at);

    let (shown, stack) = match launch.showing(focus).get(focus.path) {
        Some(path) => (values(path, ""), call_stack(path, focus, "")),
        None => {
            let missing = absent_group(launch, focus.group);

            (
                empty("", "Values", missing),
                empty("", "Call stack", missing),
            )
        }
    };

    Ok(html! {
        div class="col side" id=(id) {
            div class="hold grow" { (shown) }
            div class="hold grow" { (stack) }
        }
    })
}

fn values(path: &state::PathRow, id: &str) -> Markup {
    html! {
        figure class="widget" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " Values " }
                span class="note mono" { " " (path.name) " " }
            }
            @if path.values.is_empty() {
                p class="empty" { "Nothing in scope here." }
            } @else {
                ul class="vars" {
                    @for value in &path.values {
                        @let (shown, tone) = match &value.shown {
                            Some(shown) => (shown.clone(), ""),
                            None => ("not available here".to_string(), "gone"),
                        };
                        li class="across base" {
                            span class="name mono clip"
                                 title=(format!("declared at {}", value.at)) {
                                " " (value.name) " "
                            }
                            span class=(format!("grew mono clip {tone}")) title=(shown) {
                                " " (shown) " "
                            }
                            span class="type mono clip" { " " (value.type_name) " " }
                            span class=(format!("pill mono {}", kind_name(value.kind))) {
                                " " (kind_name(value.kind)) " "
                            }
                        }
                    }
                }
            }
        }
    }
}

fn call_stack(path: &state::PathRow, focus: &state::Focus, id: &str) -> Markup {
    html! {
        figure class="widget" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " Call stack " }
                span class="note mono" {
                    " " (plural(path.frames.len() as u64, "frame")) " "
                }
            }
            @if path.frames.is_empty() {
                p class="empty" { "Not executing." }
            } @else {
                ol class="frames" {
                    @for (depth, frame) in path.frames.iter().enumerate() {
                        @let (name, tone) = match frame.name.is_empty() {
                            true => ("unnamed".to_string(), "gone"),
                            false => (frame.name.clone(), ""),
                        };
                        li class="across base" aria-current=(flag(depth == focus.frame)) {
                            span class="at mono" { " " (depth) " " }
                            b class=(format!("grew clip {tone}"))
                              title=(site_text(frame.at.as_ref())) {
                                " " (name) " "
                            }
                            span class="site mono" {
                                " " (site_label(frame.at.as_ref(), "")) " "
                            }
                        }
                    }
                }
            }
        }
    }
}

fn dock(launch: &state::Pane, focus: &state::Focus, at: usize) -> Result<Markup> {
    let id = ident("dock", at);
    let held = launch.diagnostics.get(focus.finding);
    let found = findings(launch, focus, &format!("findings-{at}"));

    Ok(html! {
        div class="dockbody" id=(id) {
            div class="hold grow" { (found) }
            @if let Some(finding) = held {
                div class="hold aside" { (details(finding, "")) }
            }
        }
    })
}

fn findings(launch: &state::Pane, focus: &state::Focus, id: &str) -> Markup {
    html! {
        figure class="widget" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " Diagnostics " }
            }
            @if launch.diagnostics.is_empty() {
                p class="empty" { "No problems found." }
            } @else {
                ul class="findings" {
                    @for (index, finding) in launch.diagnostics.iter().enumerate() {
                        @let selected = index == focus.finding;
                        li class=(severity_name(finding.severity))
                           aria-current=(flag(selected))
                           data-pick=(format!("finding:{index}")) {
                            div class="top across base" {
                                b class="clip" title=(finding.headline) {
                                    " " (finding.headline) " "
                                }
                                span class="site mono" {
                                    " " (site_label(finding.at.as_ref(), "no line information")) " "
                                }
                            }
                            @if selected {
                                span class="detail" { " " (finding.detail) " " }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn details(finding: &inspect::Diagnostic, id: &str) -> Markup {
    html! {
        figure class="widget" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " Details " }
            }
            @if finding.rows.is_empty() {
                p class="empty" { "No details for this one." }
            } @else {
                dl class="kv" {
                    @for row in &finding.rows {
                        dt { (row.label) }
                        dd class="mono" {
                            @match row.value.is_empty() {
                                true => "unknown",
                                false => (row.value),
                            }
                        }
                    }
                }
            }
        }
    }
}

fn dockbar(launch: &state::Pane, id: &str) -> Result<Markup> {
    Ok(html! {
        header class="dockbar across base" id=[(!id.is_empty()).then_some(id)] {
            span class="what" { " Diagnostics " }
            span class=(format!("pill mono {}", worst_severity(&launch.diagnostics))) {
                " " (thousands(launch.diagnostics.len() as u64)) " "
            }
        }
    })
}

fn status_bar(launch: &state::Pane, id: &str) -> Result<Markup> {
    let counts = launch.counts;
    let local = launch.local;

    let global = extent_label([
        counts[0] * local[0],
        counts[1] * local[1],
        counts[2] * local[2],
    ]);

    let progress = format!(
        "{} of {}",
        thousands(launch.dispatched),
        thousands(launch.total())
    );

    Ok(html! {
        footer class="statusbar mono across" id=[(!id.is_empty()).then_some(id)] {
            span class=(format!("state {}", status_name(launch.status))) {
                " " (launch.status.label()) " "
            }
            span { " global " b { " " (global) " " } " " }
            span { " local " b { " " (extent_label(local)) " " } " " }
            span { " group " b { " " (progress) " " } " " }
            span class=(format!("pill mono {}", worst_severity(&launch.diagnostics))) {
                " " (thousands(launch.diagnostics.len() as u64)) " "
            }
        }
    })
}

fn empty(id: &str, title: &str, message: &str) -> Markup {
    html! {
        figure class="widget" id=[(!id.is_empty()).then_some(id)] {
            figcaption {
                span class="what" { " " (title) " " }
            }
            p class="empty" { (message) }
        }
    }
}

fn absent_group(launch: &state::Pane, group: u64) -> &'static str {
    match group < launch.dispatched {
        true => "This work group has run, its work items are no longer held.",
        false => "This work group has not started.",
    }
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

fn source_mark(mark: Option<state::SourceMark>) -> &'static str {
    match mark {
        Some(state::SourceMark::Current) => "current",
        Some(state::SourceMark::Fault) => "fault",
        None => "",
    }
}

fn control_name(control: state::Control) -> &'static str {
    match control {
        state::Control::Resume => "resume",
        state::Control::Step => "step",
        state::Control::Stop => "stop",
    }
}

fn control_icon(control: state::Control) -> &'static str {
    match control {
        state::Control::Resume => RESUME,
        state::Control::Step => STEP,
        state::Control::Stop => STOP,
    }
}

fn status_name(state: state::Status) -> &'static str {
    match state {
        state::Status::Ready => "ready",
        state::Status::Running => "running",
        state::Status::Paused => "paused",
        state::Status::Faulted => "faulted",
        state::Status::Finished => "finished",
    }
}

fn life_name(life: inspect::Life) -> &'static str {
    match life {
        inspect::Life::Pending => "pending",
        inspect::Life::Running => "running",
        inspect::Life::Parked => "parked",
        inspect::Life::Done => "done",
    }
}

fn kind_name(kind: inspect::Kind) -> &'static str {
    match kind {
        inspect::Kind::Uniform => "uniform",
        inspect::Kind::Affine => "affine",
        inspect::Kind::Divergent => "divergent",
        inspect::Kind::Shared => "shared",
    }
}

fn severity_name(severity: inspect::Severity) -> &'static str {
    match severity {
        inspect::Severity::Error => "error",
        inspect::Severity::Warning => "warning",
    }
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
