const GUARDED = ["hidden", "class", "aria-current", "aria-selected"];
const RETRY = 1000;
const SCOPES = { page: ".page", pane: ".pane" };

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

function fragment(event) {
    const cut = event.data.indexOf("\n");
    if (cut < 0) return;

    apply(event.data.slice(0, cut), event.data.slice(cut + 1));
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
    fetch("/fragment/page")
        .then((answer) => (answer.ok ? answer.text() : null))
        .then((html) => html && apply("page", html))
        .catch(() => {});
}

function keep(name, node) {
    return !(node.id && GUARDED.includes(name));
}

function link(live) {
    document.documentElement.dataset.link = live ? "live" : "gone";
}

function posting(event) {
    const el = event.target.closest("[data-post], [data-pick]");
    if (!el || el.disabled) return;

    const where = route(el);
    if (!where) return;

    fetch(where, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: body(el),
    }).catch(() => {});
}

function route(el) {
    return el.dataset.post || (el.dataset.pick ? "/select" : null);
}

function body(el) {
    const send = sent(el);
    if (send.includes("launch=")) return send;

    const pane = el.closest("[data-launch]");
    return pane ? `launch=${pane.dataset.launch}&${send}` : send;
}

function sent(el) {
    if (el.dataset.send) return el.dataset.send;

    return (el.dataset.pick || "")
        .split(/\s+/)
        .filter(Boolean)
        .map(field)
        .filter(Boolean)
        .join("&");
}

function field(one) {
    const cut = one.indexOf(":");
    if (cut < 0) return null;

    const channel = one.slice(0, cut);
    const key = one.slice(cut + 1);

    if (channel === "tab") return `launch=${key}&tab=`;
    if (channel === "lane") {
        const [group, lane] = key.split("/");
        return `group=${group}&lane=${lane}`;
    }

    return `${channel}=${key}`;
}

function tabbing(page, event) {
    const pick = event.target.closest('[data-pick^="tab:"]');
    if (!pick || !page.contains(pick)) return;

    select(page, pick.dataset.pick.slice(4));
}

function select(page, key) {
    page.querySelectorAll("[data-show]").forEach((el) => {
        el.hidden = el.dataset.show !== `tab:${key}`;
    });

    page.querySelectorAll('[data-mark^="tab:"]').forEach((el) => {
        el.classList.toggle("on", el.dataset.mark === `tab:${key}`);
    });

    page.querySelectorAll('[role="tab"]').forEach((el) => {
        el.setAttribute("aria-selected", String(el.dataset.pick === `tab:${key}`));
    });
}

function gripping(grip, event) {
    const owner = grip.closest(SCOPES[grip.dataset.scope] || ".page");
    const wide = grip.classList.contains("wide");
    const name = grip.dataset.drive;
    const sign = Number(grip.dataset.sign);
    const floor = floorOf(grip, owner);
    if (!Number.isFinite(floor)) return;

    const page = grip.closest(".page");
    const room = wide ? page.clientWidth : page.clientHeight;
    if (!room) return;

    const start = wide ? event.clientX : event.clientY;
    const held = parseFloat(getComputedStyle(owner).getPropertyValue(name));
    if (!Number.isFinite(held)) return;

    const move = (moved) => {
        const shift = ((wide ? moved.clientX : moved.clientY) - start) * sign;
        const want = Math.min(Math.max(floor, held + shift), room - floor - 160);

        owner.style.setProperty(name, `${Math.round(want)}px`);
    };

    const drop = () => {
        grip.classList.remove("held");
        grip.removeEventListener("pointermove", move);
        grip.removeEventListener("pointerup", drop);
        grip.removeEventListener("pointercancel", drop);
    };

    grip.classList.add("held");
    grip.setPointerCapture(event.pointerId);
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", drop);
    grip.addEventListener("pointercancel", drop);
    event.preventDefault();
}

function floorOf(grip, owner) {
    const held = Number(grip.dataset.floor);
    if (Number.isFinite(held)) return held;

    const start = parseFloat(getComputedStyle(owner).getPropertyValue(grip.dataset.drive));
    if (!Number.isFinite(start)) return NaN;

    grip.dataset.floor = String(Math.round(start));

    return Math.round(start);
}

function zooming(lattice, pane, block, event) {
    const cell = event.target.closest(".cell");
    if (!cell) return;

    shut(pane);

    const leader = Number(cell.dataset.pick.split(":")[1]);
    if (!Number.isFinite(leader)) return;

    (lattice.closest(".hold") || pane).append(zoomed(lattice, leader, block));
}

function zoomed(lattice, leader, block) {
    const stride = Number(lattice.dataset.stride);

    const close = make("button", {
        className: "close",
        type: "button",
        title: "Close",
        ariaLabel: "Close",
        textContent: "×",
    });

    const head = make("div", { className: "head" }, [
        make("span", {
            className: "what",
            textContent: `${block} x ${block} groups from ${leader}`,
        }),
        close,
    ]);

    const grid = make(
        "div",
        { className: "lattice zoom" },
        Array.from({ length: block * block }, (_, at) =>
            spot(lattice, leader + Math.floor(at / block) * stride + (at % block)),
        ),
    );

    grid.style.setProperty("--x", String(block));

    const zoom = make("div", { className: "magnifier" }, [head, grid]);

    close.addEventListener("click", () => zoom.remove());

    return zoom;
}

function spot(lattice, index) {
    const twin = lattice.querySelector(`.cell[data-pick="group:${index}"]`);

    const cell = make(
        "button",
        { className: twin ? twin.className : "cell pending", title: `group index ${index}` },
        [make("b", { className: "mono", textContent: String(index) })],
    );

    cell.style.cssText = twin ? twin.style.cssText : "";
    cell.dataset.pick = `group:${index}`;

    return cell;
}

function make(tag, props, children = []) {
    const el = Object.assign(document.createElement(tag), props);

    el.append(...children);

    return el;
}

function dismissing(event) {
    const inside = event.target.closest(".lattice.groups") || event.target.closest(".magnifier");

    if (!inside) document.querySelectorAll(".pane").forEach(shut);
}

function escaping(event) {
    if (event.key === "Escape") document.querySelectorAll(".pane").forEach(shut);
}

function shut(pane) {
    pane.querySelectorAll(".magnifier").forEach((one) => one.remove());
}

link(false);
connect();

document.addEventListener("click", posting);

document
    .querySelectorAll(".page")
    .forEach((page) => page.addEventListener("click", (event) => tabbing(page, event)));

document
    .querySelectorAll(".page .grip[data-drive]")
    .forEach((grip) => grip.addEventListener("pointerdown", (event) => gripping(grip, event)));

[...document.querySelectorAll(".lattice.groups[data-block]")]
    .filter((lattice) => Number(lattice.dataset.block) >= 2)
    .forEach((lattice) => {
        const block = Number(lattice.dataset.block);
        const pane = lattice.closest(".pane") || lattice;

        lattice.addEventListener("click", (event) => zooming(lattice, pane, block, event));
    });

document.addEventListener("keydown", escaping);
document.addEventListener("click", dismissing);
