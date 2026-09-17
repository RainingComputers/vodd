use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::time::Instant;

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const SCRIPT: &str = "text/javascript; charset=utf-8";
const PLAIN: &str = "text/plain; charset=utf-8";

const BACKLOG: usize = 64;
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
        let _read = std::io::Read::read_to_string(wire.as_reader(), &mut body);

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

    // TODO: this function is a smell?
    pub fn sent(&self, name: &str) -> bool {
        self.form.contains_key(name)
    }

    pub fn answer(self, reply: Reply) {
        let kind = tiny_http::Header::from_bytes(&b"Content-Type"[..], reply.kind.as_bytes())
            .expect("content type");

        let answer = tiny_http::Response::from_string(reply.body)
            .with_status_code(reply.code)
            .with_header(kind);

        let _sent = self.wire.respond(answer);
    }

    pub fn feed(self, inbox: Receiver<String>) {
        let mut writer = self.wire.into_writer();
        let mut alive = writer.write_all(STREAM.as_bytes()).is_ok();

        while alive {
            let Ok(piece) = inbox.recv() else {
                break;
            };

            alive = writer.write_all(piece.as_bytes()).is_ok() && writer.flush().is_ok();
        }
    }
}

type Draw = Box<dyn Fn() -> Reply + Send + Sync>;
type Piece = Box<dyn Fn(&str, usize) -> Reply + Send + Sync>;
type Listen = Box<dyn Fn() -> Option<(u64, Receiver<String>)> + Send + Sync>;
type Leave = Box<dyn Fn(u64) + Send + Sync>;
type Act = Box<dyn Fn(&Request) -> Option<Reply> + Send + Sync>;
type Note = Box<dyn Fn(&str) -> Reply + Send + Sync>;

pub struct Wiring {
    pub style: &'static str,
    pub page: Draw,
    pub fragment: Piece,
    pub listen: Listen,
    pub leave: Leave,
    pub act: Act,
    pub missing: Note,
    pub gone: Draw,
}

pub struct Socket {
    listener: tiny_http::Server,
}

impl Socket {
    pub fn bind(address: &str) -> Result<Socket, String> {
        tiny_http::Server::http(address)
            .map(|listener| Socket { listener })
            .map_err(|error| error.to_string())
    }

    pub fn detach(self, name: &str, wiring: Wiring) {
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || self.serve(wiring))
            .expect("hypermedia thread");
    }

    pub fn serve(self, wiring: Wiring) {
        let wiring = Arc::new(wiring);

        for arrived in self.listener.incoming_requests() {
            let wiring = Arc::clone(&wiring);

            std::thread::spawn(move || answer(&wiring, Request::take(arrived)));
        }
    }
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
        self.subscribers.retain(|subscriber| {
            subscriber.id != id || subscriber.post.try_send(piece.to_string()).is_ok()
        });
    }

    fn send(&mut self, piece: &str) {
        self.subscribers
            .retain(|subscriber| subscriber.post.try_send(piece.to_string()).is_ok());
    }
}

pub struct Sheet<'a, K> {
    pub page: K,
    pub shared: &'a [K],
    pub each: &'a [K],
    pub panes: usize,
    pub id: &'a dyn Fn(K, usize) -> String,
    pub draw: &'a dyn Fn(K, usize) -> String,
}

// TODO: audit this struct
pub struct Hypermedia<K> {
    listeners: Listeners,
    dirty: BTreeSet<(usize, K)>,
    whole: bool,
    prompt: bool,
    flushed: Instant,
}

impl<K> Default for Hypermedia<K> {
    fn default() -> Hypermedia<K> {
        Hypermedia {
            listeners: Listeners::default(),
            dirty: BTreeSet::new(),
            whole: false,
            prompt: false,
            flushed: Instant::now(),
        }
    }
}

impl<K: Copy + Ord> Hypermedia<K> {
    pub fn soil(&mut self, at: usize, fragment: K) {
        self.dirty.insert((at, fragment));
    }

    pub fn refresh(&mut self) {
        self.whole = true;
    }

    pub fn prompt(&mut self) {
        self.prompt = true;
    }

    pub fn leave(&mut self, id: u64) {
        self.listeners.leave(id);
    }

    pub fn greet(&mut self, sheet: &Sheet<'_, K>) -> (u64, Receiver<String>) {
        let (id, inbox) = self.listeners.listen();

        for piece in self.primer(sheet) {
            self.listeners.post(id, &piece);
        }

        (id, inbox)
    }

    pub fn flush(&mut self, sheet: &Sheet<'_, K>, resting: bool) {
        if !self.due(resting) {
            return;
        }

        if self.listeners.count() == 0 {
            self.dirty.clear();
            self.whole = false;

            return;
        }

        if self.whole {
            self.whole = false;
            self.dirty.clear();

            for piece in self.primer(sheet) {
                self.listeners.send(&piece);
            }

            return;
        }

        for (at, fragment) in std::mem::take(&mut self.dirty) {
            let piece = event(&(sheet.id)(fragment, at), &(sheet.draw)(fragment, at));

            self.listeners.send(&piece);
        }
    }

    fn due(&mut self, resting: bool) -> bool {
        if !self.prompt && !resting && self.flushed.elapsed() < BEAT {
            return false;
        }

        self.prompt = false;
        self.flushed = Instant::now();

        true
    }

    fn primer(&self, sheet: &Sheet<'_, K>) -> Vec<String> {
        let mut out = vec![event("page", &(sheet.draw)(sheet.page, 0))];

        for want in sheet.shared {
            out.push(event(&(sheet.id)(*want, 0), &(sheet.draw)(*want, 0)));
        }

        for at in 0..sheet.panes {
            for want in sheet.each {
                out.push(event(&(sheet.id)(*want, at), &(sheet.draw)(*want, at)));
            }
        }

        out
    }
}

fn answer(wiring: &Wiring, request: Request) {
    if let Some(reply) = asset(request.path()) {
        return request.answer(reply);
    }

    let reply = match request.route().as_slice() {
        [""] => (wiring.page)(),
        ["style.css"] => Reply::css(wiring.style),
        ["fragment", name] => (wiring.fragment)(name, 0),
        ["fragment", name, at] => match at.parse() {
            Ok(at) => (wiring.fragment)(name, at),
            Err(_) => (wiring.missing)(request.path()),
        },
        ["events"] => return stream(wiring, request),
        _ => match (wiring.act)(&request) {
            Some(reply) => reply,
            None => (wiring.missing)(request.path()),
        },
    };

    request.answer(reply);
}

fn stream(wiring: &Wiring, request: Request) {
    let Some((id, inbox)) = (wiring.listen)() else {
        return request.answer((wiring.gone)());
    };

    request.feed(inbox);

    (wiring.leave)(id);
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
