use crate::inspect;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::mpsc::SyncSender;

pub const COLOURS: [&str; 8] = [
    "#0b6e99", "#0f7b6c", "#d9730d", "#6940a5", "#ad1a72", "#cb912f", "#e03e3e", "#4dab9a",
];

const MAX_DIAGNOSTICS: usize = 64;
const MAX_RESIDENT: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMark {
    Current,
    Fault,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceLine {
    pub number: u32,
    pub text: String,
    pub mark: Option<SourceMark>,
    pub stop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    pub path: usize,
    pub part: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub path: Option<usize>,
    pub life: inspect::Life,
    pub marked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupCells {
    pub index: u64,
    pub lanes: Vec<Cell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Resume,
    Step,
}

impl Control {
    pub fn label(self) -> &'static str {
        match self {
            Control::Resume => "Continue",
            Control::Step => "Step into",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlKey {
    pub control: Control,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ready,
    Running,
    Paused,
    Faulted,
    Finished,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Ready => "not started",
            Status::Running => "running",
            Status::Paused => "paused",
            Status::Faulted => "faulted",
            Status::Finished => "finished",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    pub group: u64,
    pub lane: Option<u64>,
    pub path: usize,
    pub frame: usize,
    pub finding: usize,
}

pub struct PathRow {
    pub name: String,
    pub colour: String,
    pub at: inspect::Site,
    pub items: u64,
    pub values: Vec<inspect::Value>,
    pub frames: Vec<inspect::Frame>,
}

pub struct Pane {
    pub name: String,
    pub status: Status,
    pub commands: Vec<ControlKey>,

    pub local: [u64; 3],
    pub counts: [u64; 3],
    pub dispatched: u64,
    pub running: Option<u64>,
    pub faulted: BTreeSet<u64>,

    pub file: String,
    pub lines: Vec<SourceLine>,

    pub paths: Vec<PathRow>,
    pub described: Option<u64>,
    pub mix: Vec<Vec<Tint>>,
    pub groups: Vec<GroupCells>,
    pub diagnostics: Vec<inspect::Diagnostic>,
}

impl Pane {
    pub fn total(&self) -> u64 {
        self.counts[0] * self.counts[1] * self.counts[2]
    }

    pub fn lanes(&self) -> u64 {
        self.local[0] * self.local[1] * self.local[2]
    }

    pub fn resident(&self) -> impl Iterator<Item = &GroupCells> {
        self.groups.iter().filter(|group| !group.lanes.is_empty())
    }

    pub fn showing(&self, focus: &Focus) -> &[PathRow] {
        match self.described == Some(focus.group) {
            true => &self.paths,
            false => &[],
        }
    }
}

#[derive(Default)]
pub struct Page {
    pub tabs: Vec<Pane>,
    pub focus: Vec<Focus>,
    pub selected: usize,
}

struct Cluster {
    at: inspect::Site,
    stack: Vec<inspect::Frame>,
    values: Vec<inspect::Value>,
    members: Vec<usize>,
}

impl Cluster {
    fn overlap(&self, held: &[usize]) -> usize {
        self.members
            .iter()
            .filter(|lane| held.contains(lane))
            .count()
    }
}

pub enum Update {
    Started,
    Group {
        index: u64,
        lanes: Vec<inspect::Lane>,
        depth: inspect::Depth,
    },
    Reported(inspect::Diagnostic),
    Trapped {
        at: Option<inspect::Site>,
        detail: String,
    },
    Finished,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Carry,
    Locals,
}

pub enum Pick {
    Pane,
    Group(u64),
    Cell(Option<u64>),
    Path(usize),
    Frame(usize),
    Finding(usize),
}

pub enum Change {
    Open(u32, Box<Pane>),
    Linger(SyncSender<()>),
    Update(u32, Update),
    Wait(u32, Option<Update>, SyncSender<Next>),
    Close(u32),
    Press(usize, Control),
    Select(usize, Pick),
    Breakpoint(usize, u32),
}

pub enum Sent {
    Flow(SyncSender<Next>, Next),
    Done(SyncSender<()>),
}

#[derive(Default)]
pub struct State {
    model: Page,
    tab_of: BTreeMap<u32, usize>,
    trails: Vec<Vec<Vec<usize>>>,
    fault: Vec<Option<u32>>,
    stepping: BTreeMap<u32, bool>,
    marks: Vec<BTreeMap<u64, BTreeSet<usize>>>,
    suppressed: Vec<bool>,
    breaks: Vec<BTreeSet<u32>>,
    parked: BTreeMap<u32, Vec<SyncSender<Next>>>,
    lingering: Vec<SyncSender<()>>,
    releasing: bool,
    outbox: Vec<Sent>,
    prompt: bool,
}

impl State {
    fn tab(&self, at: usize) -> &Pane {
        &self.model.tabs[at]
    }

    fn focus(&self, at: usize) -> Focus {
        self.model.focus[at]
    }

    pub fn model(&self) -> &Page {
        &self.model
    }

    fn holds(&self, at: usize) -> bool {
        at < self.model.tabs.len()
    }

    fn seat(&self, id: u32) -> Option<usize> {
        self.tab_of.get(&id).copied()
    }

    fn holder(&self, at: usize) -> Option<u32> {
        self.tab_of
            .iter()
            .find(|(_, seat)| **seat == at)
            .map(|(id, _)| *id)
    }

    fn breaks(&self, at: usize) -> &BTreeSet<u32> {
        &self.breaks[at]
    }

    fn blames(&self, at: usize, group: u64) -> Option<&BTreeSet<usize>> {
        self.marks[at].get(&group)
    }

    fn fault(&self, at: usize) -> Option<u32> {
        self.fault[at]
    }

    fn stepping(&self, id: u32) -> bool {
        self.stepping.get(&id).copied().unwrap_or(false)
    }

    fn suppressed(&self, at: usize) -> bool {
        self.suppressed[at]
    }

    fn trails(&self, at: usize) -> &[Vec<usize>] {
        &self.trails[at]
    }

    fn watching(&self, at: usize) -> bool {
        self.tab(at).running == Some(self.focus(at).group)
    }

    pub fn resting(&self) -> bool {
        self.model
            .tabs
            .iter()
            .all(|tab| tab.status != Status::Running)
    }

    fn opened(mut self, id: u32, tab: Box<Pane>) -> State {
        let at = self.model.tabs.len();

        self.model.tabs.push(*tab);
        self.model
            .focus
            .push(Focus { group: 0, lane: None, path: 0, frame: 0, finding: 0 });

        self.tab_of.insert(id, at);
        self.trails.push(Vec::new());
        self.marks.push(BTreeMap::new());
        self.suppressed.push(false);
        self.breaks.push(BTreeSet::new());
        self.fault.push(None);
        self.stepping.insert(id, false);

        self
    }

    fn forgot(mut self, id: u32) -> State {
        self.stepping.remove(&id);
        self.tab_of.remove(&id);

        self
    }

    fn status(mut self, at: usize, status: Status) -> State {
        self.model.tabs[at].status = status;

        self
    }

    fn running(mut self, at: usize, running: Option<u64>) -> State {
        self.model.tabs[at].running = running;

        self
    }

    fn commands(mut self, at: usize, commands: Vec<ControlKey>) -> State {
        self.model.tabs[at].commands = commands;

        self
    }

    fn described(mut self, at: usize, described: Option<u64>) -> State {
        self.model.tabs[at].described = described;

        self
    }

    fn dispatched(mut self, at: usize, dispatched: u64) -> State {
        self.model.tabs[at].dispatched = dispatched;

        self
    }

    fn file(mut self, at: usize, file: String) -> State {
        self.model.tabs[at].file = file;

        self
    }

    fn paths(mut self, at: usize, paths: Vec<PathRow>) -> State {
        self.model.tabs[at].paths = paths;

        self
    }

    fn diagnostic(mut self, at: usize, finding: inspect::Diagnostic) -> State {
        self.model.tabs[at].diagnostics.push(finding);

        self
    }

    fn faulted(mut self, at: usize, index: u64) -> State {
        self.model.tabs[at].faulted.insert(index);

        self
    }

    fn mix(mut self, at: usize, index: u64, share: Vec<Tint>) -> State {
        if let Some(slot) = self.model.tabs[at].mix.get_mut(index as usize) {
            *slot = share;
        }

        self
    }

    fn group(mut self, at: usize, index: u64, cells: Vec<Cell>, focused: u64) -> State {
        let tab = &mut self.model.tabs[at];

        match tab.groups.iter().position(|group| group.index == index) {
            Some(found) => tab.groups[found].lanes = cells,
            None => tab.groups.push(GroupCells { index, lanes: cells }),
        }

        while tab.groups.len() > MAX_RESIDENT {
            let Some(oldest) = tab
                .groups
                .iter()
                .position(|group| group.index != focused && group.index != index)
            else {
                break;
            };

            tab.groups.remove(oldest);
        }

        self
    }

    fn marked(mut self, at: usize, marked: Vec<(bool, Option<SourceMark>)>) -> State {
        for (line, (stop, mark)) in self.model.tabs[at].lines.iter_mut().zip(marked) {
            line.stop = stop;
            line.mark = mark;
        }

        self
    }

    fn focused(mut self, at: usize, focus: Focus) -> State {
        self.model.focus[at] = focus;

        self
    }

    fn selected(mut self, at: usize) -> State {
        self.model.selected = at;

        self
    }

    fn faulting(mut self, at: usize, fault: Option<u32>) -> State {
        self.fault[at] = fault;

        self
    }

    fn breaking(mut self, at: usize, breaks: BTreeSet<u32>) -> State {
        self.breaks[at] = breaks;

        self
    }

    fn blamed(mut self, at: usize, group: u64, lane: usize) -> State {
        self.marks[at].entry(group).or_default().insert(lane);

        self
    }

    fn suppress(mut self, at: usize) -> State {
        self.suppressed[at] = true;

        self
    }

    fn trailed(mut self, at: usize, trails: Vec<Vec<usize>>) -> State {
        self.trails[at] = trails;

        self
    }

    fn steps(mut self, id: u32, stepping: bool) -> State {
        self.stepping.insert(id, stepping);

        self
    }

    fn parked(mut self, id: u32, reply: SyncSender<Next>) -> State {
        self.parked.entry(id).or_default().push(reply);

        self
    }

    fn released(mut self, id: u32, next: Next) -> State {
        for reply in self.parked.remove(&id).unwrap_or_default() {
            self.outbox.push(Sent::Flow(reply, next));
        }

        self
    }

    fn lingering(mut self, reply: SyncSender<()>) -> State {
        self.lingering.push(reply);

        self
    }

    fn lingered(mut self) -> State {
        for reply in std::mem::take(&mut self.lingering) {
            self.outbox.push(Sent::Done(reply));
        }

        self
    }

    fn releasing(mut self, releasing: bool) -> State {
        self.releasing = releasing;

        self
    }

    pub fn prompted(&self) -> bool {
        self.prompt
    }

    pub fn posted(&mut self) -> Vec<Sent> {
        std::mem::take(&mut self.outbox)
    }

    pub fn prompt(mut self, prompt: bool) -> State {
        self.prompt = prompt;

        self
    }

    fn send(mut self, sent: Sent) -> State {
        self.outbox.push(sent);

        self
    }
}

pub fn state(state: State, change: Change) -> State {
    match change {
        Change::Open(id, tab) => open(state, id, tab),
        Change::Linger(reply) => linger(state, reply),
        Change::Close(id) => close(state, id),
        Change::Wait(id, update, reply) => wait(state, id, update, reply),
        Change::Press(at, control) => press(state.prompt(true), at, control),
        Change::Select(at, pick) => select(state.prompt(true), at, pick),
        Change::Breakpoint(at, line) => toggle(state.prompt(true), at, line),
        Change::Update(id, update) => match state.seat(id) {
            None => state,
            Some(at) => match update {
                Update::Started => state,
                Update::Reported(finding) => reported(state, at, finding),
                Update::Trapped { at: site, detail } => trapped(state, at, site, detail),
                Update::Finished | Update::Aborted => ended(state, at, id),
                Update::Group { index, lanes, depth } => grouped(state, at, index, lanes, depth),
            },
        },
    }
}

pub fn opening(name: &str, source: Option<&str>, local: [u64; 3], counts: [u64; 3]) -> Pane {
    let lines: Vec<SourceLine> = source
        .unwrap_or_default()
        .lines()
        .enumerate()
        .map(|(at, text)| SourceLine {
            number: at as u32 + 1,
            text: text.to_string(),
            mark: None,
            stop: false,
        })
        .collect();

    let lines = match lines.is_empty() {
        true => vec![SourceLine { number: 1, text: String::new(), mark: None, stop: false }],
        false => lines,
    };

    let at = inspect::Site {
        file: String::new(),
        line: lines[0].number,
        column: 1,
        text: Some(lines[0].text.clone()),
    };

    Pane {
        name: name.to_string(),
        status: Status::Ready,
        commands: commands(Status::Ready, false),
        local,
        counts,
        dispatched: 0,
        running: None,
        faulted: BTreeSet::new(),
        file: String::new(),
        lines,
        paths: vec![PathRow {
            name: "P0".to_string(),
            colour: COLOURS[0].to_string(),
            at,
            items: 1,
            values: Vec::new(),
            frames: Vec::new(),
        }],
        described: None,
        mix: vec![Vec::new(); (counts[0] * counts[1] * counts[2]) as usize],
        groups: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn open(state: State, id: u32, tab: Box<Pane>) -> State {
    let tab = Pane {
        status: Status::Paused,
        commands: commands(Status::Paused, false),
        ..*tab
    };

    state.opened(id, Box::new(tab))
}

fn linger(state: State, reply: SyncSender<()>) -> State {
    match state.releasing {
        true => state.send(Sent::Done(reply)),
        false => state.lingering(reply),
    }
}

fn close(state: State, id: u32) -> State {
    let state = state.released(id, Next::Carry);

    match state.seat(id) {
        None => state.forgot(id),
        Some(at) => {
            let status = match state.tab(at).status {
                Status::Running | Status::Paused | Status::Ready => Status::Finished,
                settled => settled,
            };

            state
                .forgot(id)
                .status(at, status)
                .running(at, None)
                .commands(at, commands(status, false))
        }
    }
}

fn wait(state: State, id: u32, update: Option<Update>, reply: SyncSender<Next>) -> State {
    let struck = update
        .as_ref()
        .is_some_and(|update| strikes(&state, id, update));

    let detailed = match &update {
        Some(update) => matches!(update, Update::Group { depth, .. } if depth.detailed()),
        None => true,
    };

    let state = match update {
        Some(update) => self::state(state, Change::Update(id, update)),
        None => state,
    };

    hold(state, id, struck, detailed, reply)
}

fn press(state: State, at: usize, control: Control) -> State {
    match state.holder(at) {
        None => state,
        Some(id) if state.tab(at).status == Status::Finished => {
            state.releasing(true).lingered().released(id, Next::Carry)
        }
        Some(id) => {
            let state = state
                .steps(id, control == Control::Step)
                .status(at, Status::Running);

            settle(state, at).released(id, Next::Carry)
        }
    }
}

fn select(state: State, at: usize, pick: Pick) -> State {
    if !state.holds(at) {
        return state;
    }

    let focus = state.focus(at);

    match pick {
        Pick::Pane => state.selected(at),
        Pick::Cell(lane) => state.focused(at, Focus { lane, ..focus }),
        Pick::Frame(frame) => state.focused(at, Focus { frame, ..focus }),
        Pick::Finding(finding) => state.focused(at, Focus { finding, ..focus }),
        Pick::Path(path) => remark(state.focused(at, Focus { path, ..focus }), at),
        Pick::Group(group) => {
            let state = state.focused(at, Focus { group, lane: None, ..focus });

            remark(settle(state, at), at)
        }
    }
}

fn toggle(state: State, at: usize, line: u32) -> State {
    if !state.holds(at) {
        return state;
    }

    let breaks = state.breaks(at);

    let breaks = match breaks.contains(&line) {
        true => breaks
            .iter()
            .copied()
            .filter(|seat| *seat != line)
            .collect(),
        false => breaks
            .iter()
            .copied()
            .chain(std::iter::once(line))
            .collect(),
    };

    remark(state.breaking(at, breaks), at)
}

fn reported(state: State, at: usize, finding: inspect::Diagnostic) -> State {
    let state = match finding.blamed {
        Some((group, lane)) => state.blamed(at, group, lane),
        None => state,
    };

    if state.tab(at).diagnostics.len() >= MAX_DIAGNOSTICS {
        return match state.suppressed(at) {
            true => state,
            false => state.suppress(at).diagnostic(at, overflowed()),
        };
    }

    let state = match &finding.at {
        Some(site) if state.tab(at).file.is_empty() => {
            let file = site.file.clone();

            state.file(at, file)
        }
        _ => state,
    };

    state.diagnostic(at, finding)
}

fn trapped(state: State, at: usize, site: Option<inspect::Site>, detail: String) -> State {
    let line = site.as_ref().map(|site| site.line);

    let state = match state.tab(at).running {
        Some(index) => state.faulted(at, index),
        None => state,
    };

    let fault = inspect::Diagnostic {
        severity: inspect::Severity::Error,
        headline: detail.clone(),
        detail,
        at: site,
        rows: Vec::new(),
        blamed: None,
    };

    let state = reported(state.faulting(at, line), at, fault);

    remark(settle(state.status(at, Status::Faulted), at), at)
}

fn ended(state: State, at: usize, id: u32) -> State {
    let state = state
        .steps(id, false)
        .status(at, Status::Finished)
        .running(at, None);

    settle(state, at)
}

fn grouped(
    state: State,
    at: usize,
    index: u64,
    lanes: Vec<inspect::Lane>,
    depth: inspect::Depth,
) -> State {
    let done = depth == inspect::Depth::Ended;
    let state = landed(state, at, index, lanes);
    let dispatched = state.tab(at).dispatched.max(index + 1);

    let state = state
        .dispatched(at, dispatched)
        .running(at, (!done).then_some(index));

    let state = match done {
        true => remark(state.described(at, None), at),
        false => state,
    };

    settle(state, at)
}

fn hold(state: State, id: u32, struck: bool, detailed: bool, reply: SyncSender<Next>) -> State {
    let Some(at) = state.seat(id) else {
        return state.send(Sent::Flow(reply, Next::Carry));
    };

    let finished = state.tab(at).status == Status::Finished;

    let state = match struck && !state.watching(at) {
        true => follow(state, at),
        false => state,
    };

    if !finished && !state.watching(at) {
        return running(state, at).send(Sent::Flow(reply, Next::Carry));
    }

    let arriving = !finished && (struck || state.stepping(id));

    if arriving && !detailed {
        return state.send(Sent::Flow(reply, Next::Locals));
    }

    let state = match arriving {
        true => settle(state.steps(id, false).status(at, Status::Paused), at),
        false => state,
    };

    let holding = matches!(state.tab(at).status, Status::Paused | Status::Finished);

    match (holding, detailed) {
        (false, _) => state.send(Sent::Flow(reply, Next::Carry)),
        (true, false) => state.send(Sent::Flow(reply, Next::Locals)),
        (true, true) => state.parked(id, reply),
    }
}

fn strikes(state: &State, id: u32, update: &Update) -> bool {
    match (state.seat(id), update) {
        (Some(at), Update::Group { lanes, .. }) => lanes.iter().any(|landing| {
            landing
                .at
                .as_ref()
                .is_some_and(|site| state.breaks(at).contains(&site.line))
        }),
        _ => false,
    }
}

fn follow(state: State, at: usize) -> State {
    match state.tab(at).running {
        None => state,
        Some(group) => {
            let focus = Focus { group, lane: None, path: 0, frame: 0, ..state.focus(at) };

            state.focused(at, focus)
        }
    }
}

fn running(state: State, at: usize) -> State {
    match state.tab(at).status == Status::Running {
        true => state,
        false => settle(state.status(at, Status::Running), at),
    }
}

fn settle(state: State, at: usize) -> State {
    let commands = commands(state.tab(at).status, state.watching(at));

    state.commands(at, commands)
}

fn commands(status: Status, watching: bool) -> Vec<ControlKey> {
    let stopped = matches!(status, Status::Paused | Status::Faulted);
    let ended = status == Status::Finished;

    vec![
        ControlKey {
            control: Control::Resume,
            enabled: (stopped && watching) || ended,
        },
        ControlKey { control: Control::Step, enabled: stopped && watching },
    ]
}

fn landed(state: State, at: usize, index: u64, lanes: Vec<inspect::Lane>) -> State {
    let together = partition(&lanes);

    let state = match together.is_empty() {
        true => state,
        false => state.described(at, Some(index)),
    };

    let (state, slots) = rethread(state, at, &together);

    let cells: Vec<Cell> = lanes
        .iter()
        .enumerate()
        .map(|(lane, landing)| Cell {
            path: together
                .iter()
                .position(|group| group.members.contains(&lane))
                .and_then(|group| slots[group]),
            life: landing.life,
            marked: state
                .blames(at, index)
                .is_some_and(|struck| struck.contains(&lane)),
        })
        .collect();

    let total = cells.len().max(1) as f32;

    let share: Vec<Tint> = cells
        .iter()
        .filter_map(|cell| cell.path)
        .fold(BTreeMap::<usize, u64>::new(), |mut tally, path| {
            *tally.entry(path).or_default() += 1;

            tally
        })
        .into_iter()
        .map(|(path, count)| Tint { path, part: count as f32 / total })
        .collect();

    let focused = state.focus(at).group;

    state.mix(at, index, share).group(at, index, cells, focused)
}

fn partition(lanes: &[inspect::Lane]) -> Vec<Cluster> {
    let mut together: Vec<Cluster> = Vec::new();

    for (lane, landing) in lanes.iter().enumerate() {
        let Some(site) = &landing.at else {
            continue;
        };

        match together.iter_mut().find(|held| held.at == *site) {
            Some(held) => held.members.push(lane),
            None => together.push(Cluster {
                at: site.clone(),
                stack: landing.stack.clone(),
                values: landing.values.clone(),
                members: vec![lane],
            }),
        }
    }

    together
}
fn rethread(state: State, at: usize, together: &[Cluster]) -> (State, Vec<Option<usize>>) {
    if together.is_empty() {
        return (state, Vec::new());
    }

    let state = match state.tab(at).file.is_empty() {
        true => {
            let file = together[0].at.file.clone();

            state.file(at, file)
        }
        false => state,
    };

    let mut trails = state.trails(at).to_vec();
    let mut taken = vec![false; trails.len()];
    let mut ident: Vec<Option<usize>> = vec![None; together.len()];

    for (group, cluster) in together.iter().enumerate() {
        let best = trails
            .iter()
            .enumerate()
            .filter(|(seat, _)| !taken[*seat])
            .map(|(seat, lanes)| (cluster.overlap(lanes), std::cmp::Reverse(seat)))
            .filter(|(shared, _)| *shared > 0)
            .max();

        if let Some((_, std::cmp::Reverse(seat))) = best {
            taken[seat] = true;
            ident[group] = Some(seat);
        }
    }

    for slot in ident.iter_mut().filter(|slot| slot.is_none()) {
        match taken.iter().position(|seat| !seat) {
            Some(seat) => {
                taken[seat] = true;
                *slot = Some(seat);
            }
            None if trails.len() < COLOURS.len() => {
                trails.push(Vec::new());
                taken.push(true);
                *slot = Some(trails.len() - 1);
            }
            None => {}
        }
    }

    for lanes in trails.iter_mut() {
        lanes.clear();
    }

    let mut slots: Vec<Option<usize>> = vec![None; together.len()];
    let mut paths = Vec::with_capacity(together.len());

    for (group, seat) in ident.iter().enumerate() {
        let Some(seat) = *seat else {
            continue;
        };

        trails[seat] = together[group].members.clone();
        slots[group] = Some(paths.len());

        paths.push(PathRow {
            name: format!("P{seat}"),
            colour: COLOURS[seat].to_string(),
            at: together[group].at.clone(),
            items: together[group].members.len() as u64,
            values: together[group].values.clone(),
            frames: together[group].stack.clone(),
        });
    }

    let state = state.trailed(at, trails);

    if paths.is_empty() {
        return (state, slots);
    }

    let last = paths.len() - 1;
    let deepest = paths
        .iter()
        .map(|path| path.frames.len())
        .max()
        .unwrap_or(0);

    let focus = Focus {
        path: state.focus(at).path.min(last),
        frame: state.focus(at).frame.min(deepest.saturating_sub(1)),
        ..state.focus(at)
    };

    let state = state.paths(at, paths).focused(at, focus);

    (remark(state, at), slots)
}

fn remark(state: State, at: usize) -> State {
    let watched = state.tab(at).described == Some(state.focus(at).group);

    let current = match watched {
        true => state
            .tab(at)
            .paths
            .get(state.focus(at).path)
            .map(|path| path.at.line),
        false => None,
    };

    let fault = state.fault(at);
    let breaks = state.breaks(at);

    let marked: Vec<(bool, Option<SourceMark>)> = state
        .tab(at)
        .lines
        .iter()
        .map(|line| {
            let mark = match (Some(line.number) == fault, Some(line.number) == current) {
                (true, _) => Some(SourceMark::Fault),
                (false, true) => Some(SourceMark::Current),
                (false, false) => None,
            };

            (breaks.contains(&line.number), mark)
        })
        .collect();

    state.marked(at, marked)
}

fn overflowed() -> inspect::Diagnostic {
    inspect::Diagnostic {
        severity: inspect::Severity::Warning,
        headline: format!("more than {MAX_DIAGNOSTICS} diagnostics, the rest are counted"),
        detail: "The kernel is still checked in full, only this panel stops growing.".to_string(),
        at: None,
        rows: Vec::new(),
        blamed: None,
    }
}
