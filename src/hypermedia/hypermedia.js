const GUARDED = ["hidden", "class", "aria-current", "aria-selected"];
const RETRY = 1000;

let root = null;

function connect() {
    const stream = new EventSource("/events");

    stream.addEventListener("fragment", fragment);
    stream.addEventListener("open", () => link(true));

    stream.addEventListener("error", () => {
        link(false);

        if (stream.readyState !== EventSource.CLOSED) return;

        setTimeout(connect, RETRY);
    });
}

function posting(event) {
    const el = event.target.closest("[data-post]");
    if (!el || el.disabled) return;

    send(el.dataset.post, el.dataset.send || "");
}

function send(where, body) {
    fetch(where, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body,
    }).catch(() => {});
}

function fragment(event) {
    const cut = event.data.indexOf("\n");
    if (cut < 0) return;

    const id = event.data.slice(0, cut);

    if (root === null) root = id;

    apply(id, event.data.slice(cut + 1));
}

function apply(id, html) {
    const target = document.getElementById(id);

    if (!target) {
        resync();
        return;
    }

    Idiomorph.morph(target, html, {
        morphStyle: "outerHTML",
        callbacks: { beforeAttributeUpdated: (name, node) => keep(name, node) },
    });
}

function resync() {
    if (root === null) return;

    fetch(`/fragment/${root}`)
        .then((answer) => (answer.ok ? answer.text() : null))
        .then((html) => html && apply(root, html))
        .catch(() => {});
}

function keep(name, node) {
    return !(node.id && GUARDED.includes(name));
}

function link(live) {
    document.documentElement.dataset.link = live ? "live" : "gone";
}

link(false);
connect();

document.addEventListener("click", posting);

window.hypermedia = { send };
