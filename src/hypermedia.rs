use crate::logger;
use crate::logger::Level;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::SendError;
use std::sync::mpsc::Sender;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TrySendError;
use std::sync::mpsc::channel;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const SCRIPT: &str = "text/javascript; charset=utf-8";
const PLAIN: &str = "text/plain; charset=utf-8";

const BACKLOG: usize = 64;
const TICK: Duration = Duration::from_millis(50);
const BEAT: Duration = Duration::from_millis(250);

const CLIENT: &str = include_str!("hypermedia/hypermedia.js");
const IDIOMORPH: &str = include_str!("hypermedia/idiomorph.js");

pub const SCRIPTS: &str = r#"<script src="/idiomorph.js"></script>
<script src="/hypermedia.js"></script>"#;

const STREAM: &str = "HTTP/1.1 200 OK\r\n\
                      Content-Type: text/event-stream\r\n\
                      Cache-Control: no-cache\r\n\
                      Connection: keep-alive\r\n\
                      X-Accel-Buffering: no\r\n\r\n";

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

enum Wire<C> {
    Change(C),
    Shell(SyncSender<String>),
    Piece(String, SyncSender<Option<String>>),
    Listen(SyncSender<(u64, Receiver<String>)>),
    Leave(u64),
}

struct Subscriber {
    id: u64,
    post: SyncSender<String>,
}

#[derive(Default)]
struct Listeners {
    next: u64,
    subscribers: Vec<Subscriber>,
}

impl Listeners {
    fn listen(&mut self) -> (u64, Receiver<String>) {
        let (post, inbox) = sync_channel(BACKLOG);

        self.next += 1;
        self.subscribers.push(Subscriber { id: self.next, post });

        (self.next, inbox)
    }

    fn leave(&mut self, id: u64) {
        self.subscribers.retain(|subscriber| subscriber.id != id);
    }

    fn count(&self) -> usize {
        self.subscribers.len()
    }

    fn post(&mut self, id: u64, piece: &str) {
        self.subscribers
            .retain(|subscriber| subscriber.id != id || delivered(subscriber, piece));
    }

    fn send(&mut self, piece: &str) {
        self.subscribers
            .retain(|subscriber| delivered(subscriber, piece));
    }
}

pub struct Reply {
    code: u16,
    kind: &'static str,
    body: String,
}

impl Reply {
    pub fn html(body: impl Into<String>) -> Reply {
        Reply { code: 200, kind: HTML, body: body.into() }
    }

    pub fn css(body: &str) -> Reply {
        Reply { code: 200, kind: CSS, body: body.to_string() }
    }

    pub fn script(body: &str) -> Reply {
        Reply { code: 200, kind: SCRIPT, body: body.to_string() }
    }

    pub fn plain(body: &str) -> Reply {
        Reply { code: 200, kind: PLAIN, body: body.to_string() }
    }

    pub fn refused(body: &str) -> Reply {
        Reply { code: 400, kind: PLAIN, body: body.to_string() }
    }

    pub fn missing(body: impl Into<String>) -> Reply {
        Reply { code: 404, kind: HTML, body: body.into() }
    }

    pub fn gone(body: impl Into<String>) -> Reply {
        Reply { code: 503, kind: HTML, body: body.into() }
    }
}

pub struct Request {
    wire: tiny_http::Request,
    path: String,
    form: BTreeMap<String, String>,
}

impl Request {
    fn take(mut wire: tiny_http::Request) -> Request {
        let url = wire.url().to_string();
        let path = url.split('?').next().unwrap_or_default().to_string();

        let mut body = String::new();
        if let Err(error) = std::io::Read::read_to_string(wire.as_reader(), &mut body) {
            logger::log(
                Level::Warn,
                &format!("the body of a request for {path} could not be read, {error}"),
            );
        }

        let form = body
            .split('&')
            .filter_map(|pair| {
                let (name, value) = pair.split_once('=')?;
                Some((unescape(name), unescape(value)))
            })
            .collect();

        Request { wire, path, form }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn route(&self) -> Vec<&str> {
        self.path.trim_matches('/').split('/').collect()
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.form.get(name).map(String::as_str)
    }

    pub fn number(&self, name: &str) -> Option<u64> {
        self.form.get(name)?.parse().ok()
    }

    pub fn at(&self, name: &str) -> Option<usize> {
        Some(self.number(name)? as usize)
    }

    pub fn answer(self, reply: Reply) {
        let kind = tiny_http::Header::from_bytes(&b"Content-Type"[..], reply.kind.as_bytes())
            .expect("content type");

        let answer = tiny_http::Response::from_string(reply.body)
            .with_status_code(reply.code)
            .with_header(kind);

        if let Err(error) = self.wire.respond(answer) {
            logger::log(
                Level::Warn,
                &format!("a reply to {} could not be sent, {error}", self.path),
            );
        }
    }

    pub fn feed(self, inbox: Receiver<String>) {
        logger::log(Level::Info, "a browser opened the event stream");

        let mut writer = self.wire.into_writer();
        let mut alive = writer.write_all(STREAM.as_bytes()).is_ok();

        while alive {
            let Ok(piece) = inbox.recv() else {
                break;
            };

            alive = writer.write_all(piece.as_bytes()).is_ok() && writer.flush().is_ok();
        }

        logger::log(Level::Info, "a browser closed the event stream");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Html {
    Tag {
        name: String,
        attrs: Vec<(String, String)>,
        kids: Vec<Html>,
    },
    Leaf {
        id: String,
        body: String,
    },
}

pub struct ServerConfig<S, C> {
    pub style: &'static str,
    pub reduce: fn(S, C) -> S,
    pub after: fn(S) -> (S, bool),
    pub draw: fn(&S) -> Html,
    pub shell: fn(&S) -> String,
    pub serve: fn(&Request) -> (Vec<C>, Reply),
    pub missing: fn(&str) -> Reply,
    pub gone: fn() -> Reply,
}

impl<S, C> Clone for ServerConfig<S, C> {
    fn clone(&self) -> ServerConfig<S, C> {
        *self
    }
}

impl<S, C> Copy for ServerConfig<S, C> {}

pub struct Server<C> {
    post: Sender<Wire<C>>,
}

impl<C> Clone for Server<C> {
    fn clone(&self) -> Server<C> {
        Server { post: self.post.clone() }
    }
}

impl<C: Send + 'static> Server<C> {
    pub fn new<S: Send + 'static>(
        name: &str,
        address: &str,
        state: S,
        config: ServerConfig<S, C>,
    ) -> Result<Server<C>, String> {
        let listener = tiny_http::Server::http(address).map_err(|error| error.to_string())?;
        let (post, inbox) = channel();

        std::thread::Builder::new()
            .name(format!("{name}-state"))
            .spawn(move || run(inbox, state, config))
            .map_err(|error| error.to_string())?;

        let server = Server { post: post.clone() };

        std::thread::Builder::new()
            .name(format!("{name}-http"))
            .spawn(move || listen(listener, post, config))
            .map_err(|error| error.to_string())?;

        Ok(server)
    }

    pub fn post(&self, change: C) -> bool {
        self.post.send(Wire::Change(change)).is_ok()
    }
}

pub fn tag(name: &str, attrs: &[(&str, &str)], kids: Vec<Html>) -> Html {
    Html::Tag {
        name: name.to_string(),
        attrs: attrs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        kids,
    }
}

pub fn render(node: &Html) -> String {
    let mut out = String::new();

    write(node, &mut out);

    out
}

fn run<S, C>(inbox: Receiver<Wire<C>>, mut state: S, config: ServerConfig<S, C>) {
    let mut watchers = Listeners::default();
    let mut shown = (config.draw)(&state);
    let mut beat = Instant::now();

    loop {
        match inbox.recv_timeout(TICK) {
            Ok(Wire::Change(change)) => state = (config.reduce)(state, change),
            Ok(Wire::Shell(reply)) => answered("a page", reply.send((config.shell)(&state))),
            Ok(Wire::Leave(id)) => watchers.leave(id),
            Ok(Wire::Piece(id, reply)) => {
                let body = find(&(config.draw)(&state), &id).map(render);

                answered("a fragment", reply.send(body));
            }
            Ok(Wire::Listen(reply)) => {
                push(&state, config.draw, &mut watchers, &mut shown);

                let (id, inbox) = watchers.listen();

                let name = named(&shown).unwrap_or_default();

                watchers.post(id, &event(name, &render(&shown)));
                answered("an event stream", reply.send((id, inbox)));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                logger::log(
                    Level::Warn,
                    "every sender is gone, the state thread is stopping",
                );

                return;
            }
        }

        let (next, urgent) = (config.after)(state);
        state = next;

        if watchers.count() == 0 || !(urgent || beat.elapsed() >= BEAT) {
            continue;
        }

        beat = Instant::now();

        push(&state, config.draw, &mut watchers, &mut shown);
    }
}

fn push<S>(state: &S, draw: fn(&S) -> Html, watchers: &mut Listeners, shown: &mut Html) {
    let next = draw(state);
    let mut out = Vec::new();

    diff(shown, &next, &mut out);

    for (id, body) in out {
        watchers.send(&event(&id, &body));
    }

    *shown = next;
}

fn listen<S: 'static, C: Send + 'static>(
    listener: tiny_http::Server,
    post: Sender<Wire<C>>,
    config: ServerConfig<S, C>,
) {
    for arrived in listener.incoming_requests() {
        let post = post.clone();

        std::thread::spawn(move || answer(&post, config, Request::take(arrived)));
    }
}

fn answer<S, C>(post: &Sender<Wire<C>>, config: ServerConfig<S, C>, request: Request) {
    if let Some(reply) = asset(request.path()) {
        return request.answer(reply);
    }

    let reply = match request.route().as_slice() {
        [""] => match ask(post, Wire::Shell) {
            Some(body) => Reply::html(body),
            None => (config.gone)(),
        },
        ["style.css"] => Reply::css(config.style),
        ["fragment", name] => piece(post, config, name.to_string()),
        ["fragment", name, at] => match at.parse::<usize>() {
            Ok(at) => piece(post, config, format!("{name}-{at}")),
            Err(_) => (config.missing)(request.path()),
        },
        ["events"] => return stream(post, config, request),
        _ => {
            let (changes, reply) = (config.serve)(&request);

            for change in changes {
                if post.send(Wire::Change(change)).is_err() {
                    logger::log(
                        Level::Warn,
                        &format!(
                            "a change from {} was dropped, the state thread is gone",
                            request.path()
                        ),
                    );
                }
            }

            reply
        }
    };

    request.answer(reply);
}

fn piece<S, C>(post: &Sender<Wire<C>>, config: ServerConfig<S, C>, id: String) -> Reply {
    match ask(post, |reply| Wire::Piece(id.clone(), reply)) {
        Some(Some(body)) => Reply::html(body),
        Some(None) => (config.missing)(&id),
        None => (config.gone)(),
    }
}

fn stream<S, C>(post: &Sender<Wire<C>>, config: ServerConfig<S, C>, request: Request) {
    let Some((id, inbox)) = ask(post, Wire::Listen) else {
        return request.answer((config.gone)());
    };

    request.feed(inbox);

    if post.send(Wire::Leave(id)).is_err() {
        logger::log(
            Level::Warn,
            "a listener could not be retired, the state thread is gone",
        );
    }
}

fn ask<C, T: Send + 'static>(
    post: &Sender<Wire<C>>,
    make: impl FnOnce(SyncSender<T>) -> Wire<C>,
) -> Option<T> {
    let (reply, answer) = sync_channel(1);

    post.send(make(reply)).ok()?;

    answer
        .recv()
        .inspect_err(|error| {
            logger::log(
                Level::Warn,
                &format!("the state thread did not answer, {error}"),
            );
        })
        .ok()
}

fn delivered(subscriber: &Subscriber, piece: &str) -> bool {
    match subscriber.post.try_send(piece.to_string()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            logger::log(
                Level::Warn,
                "a browser fell behind the event stream and was dropped",
            );

            false
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

fn answered<T>(what: &str, sent: Result<(), SendError<T>>) {
    if sent.is_err() {
        logger::log(
            Level::Info,
            &format!("{what} was prepared but the request had already gone"),
        );
    }
}

fn asset(path: &str) -> Option<Reply> {
    match path {
        "/hypermedia.js" => Some(Reply::script(CLIENT)),
        "/idiomorph.js" => Some(Reply::script(IDIOMORPH)),
        _ => None,
    }
}

fn event(id: &str, html: &str) -> String {
    let mut out = String::with_capacity(html.len() + id.len() + 32);

    out.push_str("event: fragment\n");
    out.push_str("data: ");
    out.push_str(id);
    out.push('\n');

    for line in html.lines() {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }

    out.push('\n');

    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut bytes = text.bytes();

    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(' '),
            b'%' => {
                let high = bytes.next().and_then(|digit| (digit as char).to_digit(16));
                let low = bytes.next().and_then(|digit| (digit as char).to_digit(16));

                match (high, low) {
                    (Some(high), Some(low)) => out.push((high as u8 * 16 + low as u8) as char),
                    _ => out.push('%'),
                }
            }
            _ => out.push(byte as char),
        }
    }

    out
}

fn find<'a>(node: &'a Html, id: &str) -> Option<&'a Html> {
    if named(node) == Some(id) {
        return Some(node);
    }

    match node {
        Html::Tag { kids, .. } => kids.iter().find_map(|kid| find(kid, id)),
        Html::Leaf { .. } => None,
    }
}

fn diff(old: &Html, new: &Html, out: &mut Vec<(String, String)>) -> bool {
    if old == new {
        return false;
    }

    let mark = out.len();

    if let Some((before, after)) = alike(old, new) {
        let stirred = before
            .iter()
            .zip(after)
            .any(|(was, now)| diff(was, now, out));

        if !stirred {
            return false;
        }

        out.truncate(mark);
    }

    match named(new) {
        Some(id) => {
            out.push((id.to_string(), render(new)));

            false
        }
        None => true,
    }
}

fn alike<'a>(old: &'a Html, new: &'a Html) -> Option<(&'a [Html], &'a [Html])> {
    let (
        Html::Tag { name: was, attrs: had, kids: before },
        Html::Tag { name: now, attrs: has, kids: after },
    ) = (old, new)
    else {
        return None;
    };

    let same = was == now && had == has && before.len() == after.len();

    same.then_some((before.as_slice(), after.as_slice()))
}

fn named(node: &Html) -> Option<&str> {
    match node {
        Html::Tag { attrs, .. } => attrs
            .iter()
            .find(|(name, _)| name == "id")
            .map(|(_, value)| value.as_str()),
        Html::Leaf { id, .. } => Some(id),
    }
}

fn write(node: &Html, out: &mut String) {
    let (name, attrs, kids) = match node {
        Html::Leaf { body, .. } => return out.push_str(body),
        Html::Tag { name, attrs, kids } => (name, attrs, kids),
    };

    out.push('<');
    out.push_str(name);

    for (key, value) in attrs {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        quote(value, out);
        out.push('"');
    }

    out.push('>');

    if VOID.contains(&name.as_str()) {
        return;
    }

    for kid in kids {
        write(kid, out);
    }

    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

fn quote(text: &str, out: &mut String) {
    for one in text.chars() {
        match one {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(one),
        }
    }
}
