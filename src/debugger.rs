use crate::bitcode;
use crate::detectors;
use crate::hypermedia;
use crate::inspect;
use crate::interpreter;
use crate::logger;
use crate::logger::Level;
use crate::state;
use crate::state::Change;
use crate::state::Control;
use crate::state::Next;
use crate::state::Pick;
use crate::state::Sent;
use crate::state::State;
use crate::state::Update;
use crate::templates;

use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::mpsc::sync_channel;

static SESSION: Mutex<Option<hypermedia::Server<Change>>> = Mutex::new(None);
static TRIED: AtomicBool = AtomicBool::new(false);
static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1);

struct Link {
    id: u32,
    post: hypermedia::Server<Change>,
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
    ) -> Handle {
        let inspect = inspect::Inspector::new(module, source.clone());

        let Some(post) = session() else {
            return Handle { link: None, inspect };
        };

        let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        let tab = Box::new(state::opening(
            name,
            source.as_deref().map(String::as_str),
            local,
            counts,
        ));

        let link = post.post(Change::Open(id, tab)).then(|| Link { id, post });

        Handle { link, inspect }
    }

    pub fn linger() {
        let Some(post) = session() else {
            return;
        };

        let (reply, answer) = sync_channel(1);

        if post.post(Change::Linger(reply))
            && let Err(error) = answer.recv()
        {
            logger::log(
                Level::Warn,
                &format!("the debugger stopped before the program could wait for it, {error}"),
            );
        }
    }

    pub fn attached(&self) -> bool {
        self.link.is_some()
    }

    pub fn started(&self) {
        let Ok(()) = self.update(|| Ok::<_, Infallible>(Update::Started));
    }

    pub fn finished(&self) {
        let Ok(()) = self.update(|| Ok::<_, Infallible>(Update::Finished));
    }

    pub fn aborted(&self) {
        let Ok(()) = self.update(|| Ok::<_, Infallible>(Update::Aborted));
    }

    pub fn rest(&self) {
        let Ok(_next) = self.halt(|| Ok::<_, Infallible>(None));
    }

    pub fn opened(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
    ) -> Result<(), interpreter::Error> {
        self.update(|| {
            Ok(Update::Group {
                index,
                lanes: self.inspect.lanes(items, lanes, inspect::Depth::Detailed)?,
                depth: inspect::Depth::Detailed,
            })
        })
    }

    pub fn stepped(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
    ) -> Result<(), interpreter::Error> {
        let brief = self.halt::<interpreter::Error>(|| {
            Ok(Some(Update::Group {
                index,
                lanes: self.inspect.lanes(items, lanes, inspect::Depth::Brief)?,
                depth: inspect::Depth::Brief,
            }))
        })?;

        if brief == Next::Locals {
            self.halt::<interpreter::Error>(|| {
                Ok(Some(Update::Group {
                    index,
                    lanes: self.inspect.lanes(items, lanes, inspect::Depth::Detailed)?,
                    depth: inspect::Depth::Detailed,
                }))
            })?;
        }

        Ok(())
    }

    pub fn ended(
        &self,
        index: u64,
        items: &[interpreter::Interpreter],
        lanes: &[inspect::Progress],
    ) -> Result<(), interpreter::Error> {
        self.update(|| {
            Ok(Update::Group {
                index,
                lanes: self.inspect.lanes(items, lanes, inspect::Depth::Ended)?,
                depth: inspect::Depth::Ended,
            })
        })
    }

    pub fn report(
        &self,
        diagnostic: &detectors::Diagnostic,
        item: Option<&interpreter::Interpreter>,
        group: u64,
    ) {
        let Ok(()) = self.update(|| {
            Ok::<_, Infallible>(Update::Reported(
                self.inspect.annotate(diagnostic, item, group),
            ))
        });
    }

    pub fn trapped(&self, location: Option<bitcode::Location>, error: &dyn fmt::Debug) {
        let Ok(()) = self.update(|| {
            Ok::<_, Infallible>(Update::Trapped {
                at: self.inspect.site(location),
                detail: format!("kernel trapped: {error:?}"),
            })
        });
    }

    pub fn faulted(&self, location: Option<bitcode::Location>, error: &dyn fmt::Debug) {
        let Ok(()) = self.update(|| {
            Ok::<_, Infallible>(Update::Trapped {
                at: self.inspect.site(location),
                detail: format!("kernel faulted: {error:?}"),
            })
        });
    }

    fn halt<E>(&self, make: impl FnOnce() -> Result<Option<Update>, E>) -> Result<Next, E> {
        let Some(link) = &self.link else {
            return Ok(Next::Carry);
        };

        let (reply, answer) = sync_channel(1);

        match link.post.post(Change::Wait(link.id, make()?, reply)) {
            true => Ok(answer.recv().unwrap_or_else(|error| {
                logger::log(
                    Level::Warn,
                    &format!("the debugger stopped answering, the kernel will continue, {error}"),
                );

                Next::Carry
            })),
            false => {
                logger::log(
                    Level::Warn,
                    "the debugger is gone, the kernel will continue without it",
                );

                Ok(Next::Carry)
            }
        }
    }

    fn update<E>(&self, make: impl FnOnce() -> Result<Update, E>) -> Result<(), E> {
        let Some(link) = &self.link else {
            return Ok(());
        };

        if !link.post.post(Change::Update(link.id, make()?)) {
            logger::log(
                Level::Warn,
                "a kernel update was dropped, the debugger is no longer listening",
            );
        }

        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let Some(link) = &self.link else {
            return;
        };

        if !link.post.post(Change::Close(link.id)) {
            logger::log(
                Level::Warn,
                "a launch could not be closed, the debugger is no longer listening",
            );
        }
    }
}

fn session() -> Option<hypermedia::Server<Change>> {
    let mut session = SESSION.lock().expect("session");

    if let Some(server) = session.as_ref() {
        return Some(server.clone());
    }

    if TRIED.swap(true, Ordering::Relaxed) {
        return None;
    }

    let address = address()?;

    let server =
        match hypermedia::Server::new("vodd-debugger", &address, State::default(), config()) {
            Ok(server) => server,
            Err(error) => {
                logger::log(
                    Level::Error,
                    &format!("the debugger could not listen on {address}, {error}"),
                );

                return None;
            }
        };

    logger::log(Level::Info, &format!("debugger on http://{address}"));

    *session = Some(server.clone());

    Some(server)
}

fn config() -> hypermedia::ServerConfig<State, Change> {
    hypermedia::ServerConfig {
        style: templates::STYLE,
        reduce: state::state,
        after,
        draw,
        shell,
        serve,
        missing,
        gone,
    }
}

fn address() -> Option<String> {
    std::env::var("VODD_DEBUG").ok().filter(|at| !at.is_empty())
}

fn after(state: State) -> (State, bool) {
    let mut state = state;

    for sent in state.posted() {
        match sent {
            Sent::Flow(reply, next) => replied(reply.send(next).is_ok()),
            Sent::Done(reply) => replied(reply.send(()).is_ok()),
        }
    }

    let urgent = state.prompted() || state.resting();

    (state.prompt(false), urgent)
}

fn replied(sent: bool) {
    if !sent {
        logger::log(
            Level::Info,
            "a reply was ready but the kernel that asked for it had already gone",
        );
    }
}

fn draw(state: &State) -> hypermedia::Html {
    templates::draw(state.model())
}

fn shell(state: &State) -> String {
    templates::shell(state.model())
}

fn serve(request: &hypermedia::Request) -> (Vec<Change>, hypermedia::Reply) {
    match request.route().as_slice() {
        ["control"] => press(request),
        ["select"] => choose(request),
        ["breakpoint"] => mark(request),
        _ => (Vec::new(), missing(request.path())),
    }
}

fn press(request: &hypermedia::Request) -> (Vec<Change>, hypermedia::Reply) {
    let Some(at) = request.at("launch") else {
        return (Vec::new(), refused("a launch"));
    };

    let control = match request.field("do") {
        Some("resume") => Control::Resume,
        Some("step") => Control::Step,
        _ => return (Vec::new(), refused("a control")),
    };

    (
        vec![Change::Press(at, control)],
        hypermedia::Reply::plain("ok"),
    )
}

fn choose(request: &hypermedia::Request) -> (Vec<Change>, hypermedia::Reply) {
    let Some(at) = request.at("launch") else {
        return (Vec::new(), refused("a launch"));
    };

    let picks = [
        request.field("tab").map(|_| Pick::Pane),
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

    let changes = picks
        .into_iter()
        .flatten()
        .map(|pick| Change::Select(at, pick))
        .collect();

    (changes, hypermedia::Reply::plain("ok"))
}

fn mark(request: &hypermedia::Request) -> (Vec<Change>, hypermedia::Reply) {
    let Some(at) = request.at("launch") else {
        return (Vec::new(), refused("a launch"));
    };

    let Some(line) = request.number("line") else {
        return (Vec::new(), refused("a line"));
    };

    (
        vec![Change::Breakpoint(at, line as u32)],
        hypermedia::Reply::plain("ok"),
    )
}

fn refused(what: &str) -> hypermedia::Reply {
    hypermedia::Reply::refused(&format!("the request did not name {what}."))
}

fn missing(what: &str) -> hypermedia::Reply {
    hypermedia::Reply::missing(templates::unavailable(
        "",
        "Not found",
        &templates::missing(what),
    ))
}

fn gone() -> hypermedia::Reply {
    hypermedia::Reply::gone(templates::unavailable(
        "",
        "Gone",
        &templates::missing("the debugger session"),
    ))
}
