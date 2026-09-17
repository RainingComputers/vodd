use crate::bitcode;
use crate::detectors;
use crate::hypermedia;
use crate::inspect;
use crate::interpreter;
use crate::logger;
use crate::templates;

use serde::Serialize;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::channel;
use std::sync::mpsc::sync_channel;
use std::time::Duration;

static SESSION: Mutex<Option<Sender<Message>>> = Mutex::new(None);
static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1);

const MAX_DIAGNOSTICS: usize = 64;
const MAX_RESIDENT: usize = 1024;
const TICK: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum Error {
    Listen(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Ready,
    Running,
    Paused,
    Faulted,
    Finished,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Ready => "not started",
            State::Running => "running",
            State::Paused => "paused",
            State::Faulted => "faulted",
            State::Finished => "finished",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupCells {
    pub index: u64,
    pub lanes: Vec<Cell>,
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
    pub state: State,
    pub commands: Vec<ControlKey>,

    pub local: [u64; 3],
    pub counts: [u64; 3],
    pub dispatched: u64,
    pub running: Vec<u64>,
    pub faulted: Vec<u64>,

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

impl Page {
    pub fn open(&mut self, tab: Pane) -> usize {
        self.tabs.push(tab);
        self.focus
            .push(Focus { group: 0, lane: None, path: 0, frame: 0, finding: 0 });

        self.tabs.len() - 1
    }
}

pub enum Update {
    Started,
    Group {
        index: u64,
        lanes: Vec<inspect::Lane>,
        depth: inspect::Depth,
    },
    Reported(Box<inspect::Diagnostic>),
    Trapped {
        at: Option<inspect::Site>,
        detail: String,
    },
    Finished,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Go,
    Detail,
}

enum Pick {
    Pane,
    Group(u64),
    Cell(Option<u64>),
    Path(usize),
    Frame(usize),
    Finding(usize),
}

enum Message {
    Open(u32, Box<Pane>),
    Linger(SyncSender<()>),
    Change(u32, Box<Update>),
    Wait(u32, Option<Box<Update>>, SyncSender<Flow>),
    Close(u32),
    Leave(u64),
    Draw(templates::Fragment, usize, SyncSender<String>),
    Shell(SyncSender<String>),
    Listen(SyncSender<(u64, Receiver<String>)>),
    Press(usize, Control),
    Select(usize, Pick),
    Breakpoint(usize, u32),
}

struct Link {
    id: u32,
    post: Sender<Message>,
}

pub struct Handle {
    link: Option<Link>,
    inspect: inspect::Inspector,
}

impl Handle {
    pub fn open(
        name: &str,
        module: Arc<bitcode::Module>,
        source: Option<Arc<String>>,
        local: [u64; 3],
        counts: [u64; 3],
    ) -> Result<Handle, Error> {
        let inspect = inspect::Inspector::new(module, source.clone());

        let Some(post) = session()? else {
            return Ok(Handle { link: None, inspect });
        };

        let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        let tab = Box::new(opening(
            name,
            source.as_deref().map(String::as_str),
            local,
            counts,
        ));

        let link = match post.send(Message::Open(id, tab)) {
            Ok(()) => Some(Link { id, post }),
            Err(_) => None,
        };

        Ok(Handle { link, inspect })
    }

    pub fn attached(&self) -> bool {
        self.link.is_some()
    }

    pub fn started(&self) {
        self.update(Update::Started);
    }

    pub fn finished(&self) {
        self.update(Update::Finished);
    }

    pub fn aborted(&self) {
        self.update(Update::Aborted);
    }

    pub fn rest(&self) {
        let _flow = self.halt(None);
    }

    pub fn linger() {
        let Ok(Some(post)) = session() else {
            return;
        };

        let (reply, answer) = sync_channel(1);

        if post.send(Message::Linger(reply)).is_ok() {
            let _parked = answer.recv();
        }
    }

    pub fn opened(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
    ) -> Result<(), interpreter::Error> {
        self.snapshot(index, items, lanes, inspect::Depth::Detailed)
    }

    pub fn stepped(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
    ) -> Result<(), interpreter::Error> {
        if !self.attached() {
            return Ok(());
        }

        let brief = Update::Group {
            index,
            lanes: self.inspect.lanes(items, lanes, inspect::Depth::Brief)?,
            depth: inspect::Depth::Brief,
        };

        if self.halt(Some(brief)) == Flow::Detail {
            let full = Update::Group {
                index,
                lanes: self.inspect.lanes(items, lanes, inspect::Depth::Detailed)?,
                depth: inspect::Depth::Detailed,
            };

            self.halt(Some(full));
        }

        Ok(())
    }

    pub fn ended(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
    ) -> Result<(), interpreter::Error> {
        self.snapshot(index, items, lanes, inspect::Depth::Ended)
    }

    pub fn report(
        &self,
        diagnostic: &detectors::Diagnostic,
        item: Option<&interpreter::Interpreter>,
        group: u64,
    ) {
        if !self.attached() {
            return;
        }

        let report = self.inspect.annotate(diagnostic, item, group);

        self.update(Update::Reported(Box::new(report)));
    }

    pub fn trapped(&self, location: Option<bitcode::Location>, error: &dyn fmt::Debug) {
        self.faltered(location, "trapped", error);
    }

    pub fn faulted(&self, location: Option<bitcode::Location>, error: &dyn fmt::Debug) {
        self.faltered(location, "faulted", error);
    }

    fn halt(&self, update: Option<Update>) -> Flow {
        let Some(link) = &self.link else {
            return Flow::Go;
        };

        let (reply, answer) = sync_channel(1);

        match link
            .post
            .send(Message::Wait(link.id, update.map(Box::new), reply))
        {
            Ok(()) => answer.recv().unwrap_or(Flow::Go),
            Err(_) => Flow::Go,
        }
    }

    fn snapshot(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
        depth: inspect::Depth,
    ) -> Result<(), interpreter::Error> {
        if !self.attached() {
            return Ok(());
        }

        self.update(Update::Group {
            index,
            lanes: self.inspect.lanes(items, lanes, depth)?,
            depth,
        });

        Ok(())
    }

    fn faltered(&self, location: Option<bitcode::Location>, verb: &str, error: &dyn fmt::Debug) {
        self.update(Update::Trapped {
            at: self.inspect.site(location),
            detail: format!("kernel {verb}: {error:?}"),
        });
    }

    fn update(&self, update: Update) {
        let Some(link) = &self.link else {
            return;
        };

        let _posted = link.post.send(Message::Change(link.id, Box::new(update)));
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let Some(link) = &self.link else {
            return;
        };

        let _posted = link.post.send(Message::Close(link.id));
    }
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

// TODO: audit this struct
#[derive(Default)]
struct Session {
    model: Page,
    tab_of: BTreeMap<u32, usize>,
    trails: Vec<Vec<Vec<usize>>>,
    fault: Vec<Option<u32>>,
    stepping: BTreeMap<u32, bool>,
    marks: Vec<BTreeMap<u64, BTreeSet<usize>>>,
    suppressed: Vec<bool>,
    breaks: Vec<BTreeSet<u32>>,
    depth: Vec<usize>,
    parked: BTreeMap<u32, Vec<SyncSender<Flow>>>,
    lingering: Vec<SyncSender<()>>,
    releasing: bool,
    media: hypermedia::Hypermedia<templates::Fragment>,
}

impl Session {
    fn run(mut self, inbox: Receiver<Message>) {
        loop {
            match inbox.recv_timeout(TICK) {
                Ok(message) => self.take(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }

            let resting = self.resting();
            self.flush(resting);
        }
    }

    fn resting(&self) -> bool {
        self.model
            .tabs
            .iter()
            .all(|tab| tab.state != State::Running)
    }

    fn take(&mut self, message: Message) {
        match message {
            Message::Open(id, tab) => self.open(id, *tab),
            Message::Linger(reply) => match self.releasing {
                true => drop(reply.send(())),
                false => self.lingering.push(reply),
            },
            Message::Change(id, update) => self.change(id, *update),
            Message::Wait(id, update, reply) => {
                let (struck, detailed) = match &update {
                    Some(update) => (
                        self.struck(id, update),
                        matches!(**update, Update::Group { depth, .. } if depth.detailed()),
                    ),
                    None => (false, true),
                };

                if let Some(update) = update {
                    self.change(id, *update);
                }

                self.hold(id, struck, detailed, reply);
            }
            Message::Close(id) => self.close(id),
            Message::Draw(want, at, reply) => {
                let _sent = reply.send(draw(&self.model, want, at));
            }
            Message::Shell(reply) => {
                let _sent = reply.send(shell(&self.model));
            }
            Message::Leave(id) => self.media.leave(id),
            Message::Listen(reply) => {
                let Session { model, media, .. } = self;
                let held = media.greet(&sheet(model, &|want, at| draw(model, want, at)));

                let _sent = reply.send(held);
            }
            Message::Press(at, control) => {
                self.media.prompt();
                self.press(at, control);
            }
            Message::Select(at, pick) => {
                self.media.prompt();
                self.select(at, pick);
            }
            Message::Breakpoint(at, line) => {
                self.media.prompt();
                self.toggle(at, line);
            }
        }
    }

    fn open(&mut self, id: u32, mut tab: Pane) {
        tab.state = State::Paused;
        tab.commands = commands(State::Paused, false);

        let at = self.model.open(tab);

        self.tab_of.insert(id, at);
        self.trails.push(Vec::new());
        self.marks.push(BTreeMap::new());
        self.suppressed.push(false);
        self.breaks.push(BTreeSet::new());
        self.depth.push(0);
        self.fault.push(None);
        self.stepping.insert(id, false);
        self.media.refresh();
    }

    fn close(&mut self, id: u32) {
        self.release(id, Flow::Go);
        self.stepping.remove(&id);

        let Some(at) = self.tab_of.remove(&id) else {
            return;
        };

        let tab = &mut self.model.tabs[at];

        if matches!(tab.state, State::Running | State::Paused | State::Ready) {
            tab.state = State::Finished;
        }

        tab.running.clear();
        tab.commands = commands(tab.state, false);

        self.soil(at, templates::Fragment::Status);
        self.soil(at, templates::Fragment::Source);
        self.soil(at, templates::Fragment::Groups);
        self.soil(at, templates::Fragment::Tabs);
    }

    fn struck(&self, id: u32, update: &Update) -> bool {
        let Some(at) = self.tab_of.get(&id).copied() else {
            return false;
        };

        let Update::Group { lanes, .. } = update else {
            return false;
        };

        lanes.iter().any(|landing| {
            landing
                .at
                .as_ref()
                .is_some_and(|site| self.breaks[at].contains(&site.line))
        })
    }

    fn toggle(&mut self, at: usize, line: u32) {
        if self.breaks.get(at).is_none() {
            return;
        }

        if !self.breaks[at].remove(&line) {
            self.breaks[at].insert(line);
        }

        self.remark(at);
    }

    fn running(&mut self, at: usize) {
        if self.model.tabs[at].state == State::Running {
            return;
        }

        self.model.tabs[at].state = State::Running;
        self.settle(at);

        self.soil(at, templates::Fragment::Status);
        self.soil(at, templates::Fragment::Source);
        self.soil(at, templates::Fragment::Tabs);
    }

    fn follow(&mut self, at: usize) {
        let Some(index) = self.model.tabs[at].running.first().copied() else {
            return;
        };

        self.model.focus[at].group = index;
        self.model.focus[at].lane = None;
        self.model.focus[at].path = 0;
        self.model.focus[at].frame = 0;

        self.soil(at, templates::Fragment::Groups);
        self.soil(at, templates::Fragment::Items);
        self.soil(at, templates::Fragment::Paths);
        self.soil(at, templates::Fragment::Side);
    }

    fn watching(&self, at: usize) -> bool {
        self.model.tabs[at]
            .running
            .first()
            .is_some_and(|running| *running == self.model.focus[at].group)
    }

    fn hold(&mut self, id: u32, struck: bool, detailed: bool, reply: SyncSender<Flow>) {
        let Some(at) = self.tab_of.get(&id).copied() else {
            let _sent = reply.send(Flow::Go);
            return;
        };

        let held = matches!(self.model.tabs[at].state, State::Finished);

        if struck && !self.watching(at) {
            self.follow(at);
        }

        if !held && !self.watching(at) {
            self.running(at);

            let _sent = reply.send(Flow::Go);
            return;
        }

        let stepping = self.stepping.get(&id).copied().unwrap_or(false);
        let arriving = !held && (struck || stepping);

        if arriving && !detailed {
            let _sent = reply.send(Flow::Detail);
            return;
        }

        if arriving {
            self.stepping.insert(id, false);
            self.model.tabs[at].state = State::Paused;
            self.settle(at);
            self.soil(at, templates::Fragment::Status);
            self.soil(at, templates::Fragment::Source);
            self.soil(at, templates::Fragment::Tabs);
        }

        let holding = matches!(self.model.tabs[at].state, State::Paused | State::Finished);

        if !holding {
            let _sent = reply.send(Flow::Go);
            return;
        }

        if !detailed {
            let _sent = reply.send(Flow::Detail);
            return;
        }

        self.parked.entry(id).or_default().push(reply);
    }

    fn release(&mut self, id: u32, flow: Flow) {
        for reply in self.parked.remove(&id).unwrap_or_default() {
            let _sent = reply.send(flow);
        }
    }

    fn handle_of(&self, at: usize) -> Option<u32> {
        self.tab_of
            .iter()
            .find(|(_, held)| **held == at)
            .map(|(id, _)| *id)
    }

    fn press(&mut self, at: usize, control: Control) {
        let Some(id) = self.handle_of(at) else {
            return;
        };

        if self.model.tabs[at].state == State::Finished {
            self.releasing = true;

            for reply in self.lingering.drain(..) {
                let _sent = reply.send(());
            }

            self.release(id, Flow::Go);

            return;
        }

        self.stepping.insert(id, control == Control::Step);
        self.model.tabs[at].state = State::Running;
        self.settle(at);
        self.release(id, Flow::Go);

        self.soil(at, templates::Fragment::Status);
        self.soil(at, templates::Fragment::Source);
        self.soil(at, templates::Fragment::Tabs);
    }

    fn select(&mut self, at: usize, pick: Pick) {
        if self.model.focus.get(at).is_none() {
            return;
        }

        match pick {
            Pick::Pane => {
                self.model.selected = at;
                self.media.refresh();
            }
            Pick::Group(index) => {
                self.model.focus[at].group = index;
                self.model.focus[at].lane = None;
                self.settle(at);
                self.remark(at);
                self.soil(at, templates::Fragment::Groups);
                self.soil(at, templates::Fragment::Items);
                self.soil(at, templates::Fragment::Paths);
                self.soil(at, templates::Fragment::Side);
            }
            Pick::Cell(lane) => {
                self.model.focus[at].lane = lane;
                self.soil(at, templates::Fragment::Items);
            }
            Pick::Path(path) => {
                self.model.focus[at].path = path;
                self.soil(at, templates::Fragment::Paths);
                self.soil(at, templates::Fragment::Side);
                self.remark(at);
            }
            Pick::Frame(frame) => {
                self.model.focus[at].frame = frame;
                self.soil(at, templates::Fragment::Side);
            }
            Pick::Finding(finding) => {
                self.model.focus[at].finding = finding;
                self.soil(at, templates::Fragment::Dock);
            }
        }
    }

    fn change(&mut self, id: u32, update: Update) {
        let Some(at) = self.tab_of.get(&id).copied() else {
            return;
        };

        match update {
            Update::Started => {
                self.soil(at, templates::Fragment::Status);
                self.soil(at, templates::Fragment::Tabs);
            }
            Update::Group { index, lanes, depth } => {
                let done = depth == inspect::Depth::Ended;

                self.depth[at] = lanes
                    .iter()
                    .filter(|landing| landing.life == inspect::Life::Running)
                    .map(|landing| landing.stack.len())
                    .max()
                    .unwrap_or(0);

                self.landed(at, index, lanes);

                let tab = &mut self.model.tabs[at];
                tab.dispatched = tab.dispatched.max(index + 1);
                tab.running = match done {
                    true => Vec::new(),
                    false => vec![index],
                };

                if done {
                    self.model.tabs[at].described = None;
                    self.remark(at);
                }

                self.settle(at);
                self.soil(at, templates::Fragment::Source);
                self.soil(at, templates::Fragment::Groups);
                self.soil(at, templates::Fragment::Items);
                self.soil(at, templates::Fragment::Paths);
                self.soil(at, templates::Fragment::Status);
            }
            Update::Reported(finding) => self.reported(at, *finding),
            Update::Trapped { at: site, detail } => {
                self.fault[at] = site.as_ref().map(|site| site.line);

                if let Some(index) = self.model.tabs[at].running.first().copied() {
                    self.model.tabs[at].faulted.push(index);
                }

                self.reported(
                    at,
                    inspect::Diagnostic {
                        severity: inspect::Severity::Error,
                        headline: detail.clone(),
                        detail,
                        at: site,
                        rows: Vec::new(),
                        blamed: None,
                    },
                );

                self.model.tabs[at].state = State::Faulted;
                self.settle(at);
                self.remark(at);

                self.soil(at, templates::Fragment::Status);
                self.soil(at, templates::Fragment::Groups);
                self.soil(at, templates::Fragment::Tabs);
            }
            Update::Finished | Update::Aborted => {
                self.stepping.insert(id, false);
                self.model.tabs[at].state = State::Finished;
                self.model.tabs[at].running.clear();
                self.settle(at);
                self.soil(at, templates::Fragment::Status);
                self.soil(at, templates::Fragment::Groups);
                self.soil(at, templates::Fragment::Tabs);
            }
        }
    }

    fn settle(&mut self, at: usize) {
        let state = self.model.tabs[at].state;
        let watching = self.watching(at);

        self.model.tabs[at].commands = commands(state, watching);
    }

    fn reported(&mut self, at: usize, finding: inspect::Diagnostic) {
        if let Some((group, lane)) = finding.blamed {
            self.marks[at].entry(group).or_default().insert(lane);
            self.soil(at, templates::Fragment::Items);
        }

        if self.model.tabs[at].diagnostics.len() >= MAX_DIAGNOSTICS {
            if !self.suppressed[at] {
                self.suppressed[at] = true;

                self.model.tabs[at].diagnostics.push(inspect::Diagnostic {
                    severity: inspect::Severity::Warning,
                    headline: format!(
                        "more than {MAX_DIAGNOSTICS} diagnostics, the rest are counted"
                    ),
                    detail: "The kernel is still checked in full, only this panel stops growing."
                        .to_string(),
                    at: None,
                    rows: Vec::new(),
                    blamed: None,
                });

                self.soil(at, templates::Fragment::Dock);
                self.soil(at, templates::Fragment::Dockbar);
            }

            return;
        }

        if let Some(site) = &finding.at
            && self.model.tabs[at].file.is_empty()
        {
            self.model.tabs[at].file = site.file.clone();
            self.soil(at, templates::Fragment::Source);
        }

        self.model.tabs[at].diagnostics.push(finding);

        self.soil(at, templates::Fragment::Dock);
        self.soil(at, templates::Fragment::Dockbar);
        self.soil(at, templates::Fragment::Tabs);
    }

    fn landed(&mut self, at: usize, index: u64, lanes: Vec<inspect::Lane>) {
        let together = partition(&lanes);

        if !together.is_empty() {
            self.model.tabs[at].described = Some(index);
        }

        let slots = self.rethread(at, &together);

        let mut held = Vec::with_capacity(lanes.len());
        let mut tally: BTreeMap<usize, u64> = BTreeMap::new();

        for (lane, landing) in lanes.iter().enumerate() {
            let path = together
                .iter()
                .position(|group| group.members.contains(&lane))
                .and_then(|group| slots[group]);

            if let Some(path) = path {
                *tally.entry(path).or_default() += 1;
            }

            let marked = self.marks[at]
                .get(&index)
                .is_some_and(|struck| struck.contains(&lane));

            held.push(Cell { path, life: landing.life, marked });
        }

        let total = held.len().max(1) as f32;

        let share: Vec<Tint> = tally
            .iter()
            .map(|(path, count)| Tint { path: *path, part: *count as f32 / total })
            .collect();

        let focused = self.model.focus[at].group;
        let tab = &mut self.model.tabs[at];

        if let Some(slot) = tab.mix.get_mut(index as usize) {
            *slot = share;
        }

        match tab.groups.iter().position(|group| group.index == index) {
            Some(found) => tab.groups[found].lanes = held,
            None => tab.groups.push(GroupCells { index, lanes: held }),
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
    }

    fn rethread(&mut self, at: usize, together: &[Cluster]) -> Vec<Option<usize>> {
        let mut slots: Vec<Option<usize>> = vec![None; together.len()];

        if together.is_empty() {
            return slots;
        }

        if self.model.tabs[at].file.is_empty() {
            self.model.tabs[at].file = together[0].at.file.clone();
        }

        let mut trails = std::mem::take(&mut self.trails[at]);
        let mut taken = vec![false; trails.len()];
        let mut ident: Vec<Option<usize>> = vec![None; together.len()];

        for (group, cluster) in together.iter().enumerate() {
            let best = trails
                .iter()
                .enumerate()
                .filter(|(held, _)| !taken[*held])
                .map(|(held, lanes)| (cluster.overlap(lanes), std::cmp::Reverse(held)))
                .filter(|(shared, _)| *shared > 0)
                .max();

            if let Some((_, std::cmp::Reverse(held))) = best {
                taken[held] = true;
                ident[group] = Some(held);
            }
        }

        for slot in ident.iter_mut().filter(|slot| slot.is_none()) {
            match taken.iter().position(|held| !held) {
                Some(held) => {
                    taken[held] = true;
                    *slot = Some(held);
                }
                None if trails.len() < templates::COLOURS.len() => {
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

        let mut paths = Vec::with_capacity(together.len());

        for (group, held) in ident.iter().enumerate() {
            let Some(held) = *held else {
                continue;
            };

            trails[held] = together[group].members.clone();
            slots[group] = Some(paths.len());

            paths.push(PathRow {
                name: format!("P{held}"),
                colour: templates::COLOURS[held].to_string(),
                at: together[group].at.clone(),
                items: together[group].members.len() as u64,
                values: together[group].values.clone(),
                frames: together[group].stack.clone(),
            });
        }

        self.trails[at] = trails;

        if paths.is_empty() {
            return slots;
        }

        let last = paths.len() - 1;

        let deepest = paths
            .iter()
            .map(|path| path.frames.len())
            .max()
            .unwrap_or(0);

        self.model.tabs[at].paths = paths;
        self.model.focus[at].path = self.model.focus[at].path.min(last);
        self.model.focus[at].frame = self.model.focus[at].frame.min(deepest.saturating_sub(1));

        self.soil(at, templates::Fragment::Side);

        self.soil(at, templates::Fragment::Paths);
        self.remark(at);

        slots
    }

    fn remark(&mut self, at: usize) {
        let here = self.model.focus[at].path;
        let watched = self.model.tabs[at].described == Some(self.model.focus[at].group);

        let current = match watched {
            true => self.model.tabs[at].paths.get(here).map(|path| path.at.line),
            false => None,
        };
        let fault = self.fault[at];
        let breaks = std::mem::take(&mut self.breaks[at]);

        for line in &mut self.model.tabs[at].lines {
            line.stop = breaks.contains(&line.number);
            line.mark = match (Some(line.number) == fault, Some(line.number) == current) {
                (true, _) => Some(SourceMark::Fault),
                (false, true) => Some(SourceMark::Current),
                (false, false) => None,
            };
        }

        self.breaks[at] = breaks;

        self.soil(at, templates::Fragment::Source);
    }

    fn soil(&mut self, at: usize, fragment: templates::Fragment) {
        self.media.soil(at, fragment);
    }

    fn flush(&mut self, resting: bool) {
        let Session { model, media, .. } = self;

        media.flush(&sheet(model, &|want, at| draw(model, want, at)), resting);
    }
}

// TODO: audit this struct
#[derive(Clone)]
struct Server {
    post: Sender<Message>,
}

impl Server {
    fn wiring(&self) -> hypermedia::Wiring {
        let page = self.clone();
        let fragment = self.clone();
        let listen = self.clone();
        let leave = self.clone();
        let act = self.clone();

        hypermedia::Wiring {
            style: templates::STYLE,
            page: Box::new(move || page.drawn(Message::Shell)),
            fragment: Box::new(move |name, at| fragment.piece(name, at)),
            listen: Box::new(move || listen.ask(Message::Listen)),
            leave: Box::new(move |id| leave.tell(Message::Leave(id))),
            act: Box::new(move |request| act.act(request)),
            missing: Box::new(missing),
            gone: Box::new(gone),
        }
    }

    fn act(&self, request: &hypermedia::Request) -> Option<hypermedia::Reply> {
        match request.route().as_slice() {
            ["control"] => Some(self.press(request)),
            ["select"] => Some(self.choose(request)),
            ["breakpoint"] => Some(self.mark(request)),
            _ => None,
        }
    }

    fn ask<T: Send + 'static>(&self, make: impl FnOnce(SyncSender<T>) -> Message) -> Option<T> {
        let (reply, answer) = sync_channel(1);

        self.post.send(make(reply)).ok()?;

        answer.recv().ok()
    }

    fn tell(&self, message: Message) {
        let _posted = self.post.send(message);
    }

    fn drawn(&self, make: impl FnOnce(SyncSender<String>) -> Message) -> hypermedia::Reply {
        match self.ask(make) {
            Some(body) => hypermedia::Reply::html(body),
            None => gone(),
        }
    }

    fn press(&self, request: &hypermedia::Request) -> hypermedia::Reply {
        let Some(at) = request.at("launch") else {
            return refused("a launch");
        };

        let control = match request.field("do") {
            Some("resume") => Control::Resume,
            Some("step") => Control::Step,
            _ => return refused("a control"),
        };

        self.tell(Message::Press(at, control));

        hypermedia::Reply::plain("ok")
    }

    fn mark(&self, request: &hypermedia::Request) -> hypermedia::Reply {
        let Some(at) = request.at("launch") else {
            return refused("a launch");
        };

        let Some(line) = request.number("line") else {
            return refused("a line");
        };

        self.tell(Message::Breakpoint(at, line as u32));

        hypermedia::Reply::plain("ok")
    }

    fn choose(&self, request: &hypermedia::Request) -> hypermedia::Reply {
        let Some(at) = request.at("launch") else {
            return refused("a launch");
        };

        let picks = [
            request.sent("tab").then_some(Pick::Pane),
            request.number("group").map(Pick::Group),
            request
                .field("lane")
                .map(|lane| Pick::Cell(lane.parse().ok())),
            request.number("path").map(|path| Pick::Path(path as usize)),
            request
                .number("frame")
                .map(|frame| Pick::Frame(frame as usize)),
            request
                .number("finding")
                .map(|finding| Pick::Finding(finding as usize)),
        ];

        for pick in picks.into_iter().flatten() {
            self.tell(Message::Select(at, pick));
        }

        hypermedia::Reply::plain("ok")
    }

    fn piece(&self, name: &str, at: usize) -> hypermedia::Reply {
        match templates::Fragment::named(name) {
            Some(want) => self.drawn(|reply| Message::Draw(want, at, reply)),
            None => missing(name),
        }
    }
}

fn session() -> Result<Option<Sender<Message>>, Error> {
    let mut session = SESSION.lock().expect("session");

    if let Some(post) = session.as_ref() {
        return Ok(Some(post.clone()));
    }

    let Some(address) = address() else {
        return Ok(None);
    };

    let socket = hypermedia::Socket::bind(&address)
        .map_err(|error| Error::Listen(format!("{address}: {error}")))?;

    let (post, inbox) = channel();

    std::thread::Builder::new()
        .name("vodd-debugger".to_string())
        .spawn(move || Session::default().run(inbox))
        .expect("debugger thread");

    socket.detach("vodd-debugger-http", Server { post: post.clone() }.wiring());

    logger::log(&format!("debugger on http://{address}"));

    *session = Some(post.clone());

    Ok(Some(post))
}

fn address() -> Option<String> {
    std::env::var("VODD_DEBUG").ok().filter(|at| !at.is_empty())
}

fn opening(name: &str, source: Option<&str>, local: [u64; 3], counts: [u64; 3]) -> Pane {
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
        state: State::Ready,
        commands: commands(State::Ready, false),
        local,
        counts,
        dispatched: 0,
        running: Vec::new(),
        faulted: Vec::new(),
        file: String::new(),
        lines,
        paths: vec![PathRow {
            name: "P0".to_string(),
            colour: templates::COLOURS[0].to_string(),
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

fn commands(state: State, watching: bool) -> Vec<ControlKey> {
    let stopped = matches!(state, State::Paused | State::Faulted);
    let ended = state == State::Finished;

    vec![
        ControlKey {
            control: Control::Resume,
            enabled: (stopped && watching) || ended,
        },
        ControlKey { control: Control::Step, enabled: stopped && watching },
    ]
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

fn sheet<'a>(
    model: &Page,
    draw: &'a dyn Fn(templates::Fragment, usize) -> String,
) -> hypermedia::Sheet<'a, templates::Fragment> {
    hypermedia::Sheet {
        page: templates::Fragment::Page,
        shared: &templates::SHARED,
        each: &templates::FRAGMENTS,
        panes: model.tabs.len(),
        id: &|want: templates::Fragment, at| want.id(at),
        draw,
    }
}

fn draw(model: &Page, want: templates::Fragment, at: usize) -> String {
    templates::fragment(model, want, at)
}

fn shell(model: &Page) -> String {
    templates::shell(model)
}

fn missing(what: &str) -> hypermedia::Reply {
    hypermedia::Reply::missing(templates::unavailable(
        "",
        "Not found",
        &templates::missing(what),
    ))
}

fn refused(what: &str) -> hypermedia::Reply {
    hypermedia::Reply::refused(&format!("the request did not name {what}."))
}

fn gone() -> hypermedia::Reply {
    hypermedia::Reply::gone(templates::unavailable(
        "",
        "Gone",
        &templates::missing("the debugger session"),
    ))
}
