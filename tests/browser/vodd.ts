// TODO: audit all tests

import { ChildProcess, execFileSync, spawn } from "node:child_process";
import { createServer } from "node:net";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const HERE = __dirname;
const ROOT = resolve(HERE, "../..");

const MACOS = process.platform === "darwin";

const DRIVER = join(ROOT, "target/release");
const LIBRARY = join(DRIVER, MACOS ? "libvodd.dylib" : "libvodd.so");
const LIBRARY_PATH = MACOS ? "DYLD_LIBRARY_PATH" : "LD_LIBRARY_PATH";
const HOST = join(ROOT, "target/browser/host");
const SOURCE = join(ROOT, "tests/support/host.c");

const READY = 20_000;

export type Grid = [number, number, number] | [number, number] | [number];

export type Options = {
    kernel: string;
    entry: string;
    global: Grid;
    local: Grid;
    arguments?: string[];
    checks?: string;
    maxErrors?: number;
    launches?: number;
    port?: number;
    environment?: Record<string, string>;
};

export type Session = {
    url: string;
    port: number;
    output: () => string;
    running: () => boolean;
    stop: () => void;
};

let built = false;

function shell(command: string, args: string[]) {
    execFileSync(command, args, { cwd: ROOT, stdio: "pipe" });
}

function newer(target: string, ...sources: string[]) {
    if (!existsSync(target)) return false;

    const made = statSync(target).mtimeMs;

    return sources.every((source) => statSync(source).mtimeMs < made);
}

function sysroot() {
    if (!MACOS) return [];

    const path = execFileSync("xcrun", ["--show-sdk-path"]).toString().trim();

    return ["-isysroot", path];
}

function linking() {
    return MACOS ? ["-Wl,-undefined,dynamic_lookup"] : [`-Wl,-rpath,${DRIVER}`];
}

export function build() {
    if (built) return;

    shell("cargo", ["build", "--release"]);
    if (!existsSync(LIBRARY)) throw new Error(`${LIBRARY} was not built`);

    mkdirSync(dirname(HOST), { recursive: true });

    if (!newer(HOST, SOURCE, LIBRARY)) {
        shell("cc", [
            "-o",
            HOST,
            SOURCE,
            "-I",
            join(ROOT, "tests/vendor/OpenCL-Headers"),
            "-I",
            join(ROOT, "tests/support/include"),
            "-DCL_TARGET_OPENCL_VERSION=120",
            ...sysroot(),
            `-L${DRIVER}`,
            "-lvodd",
            ...linking(),
        ]);
    }

    built = true;
}

async function free(): Promise<number> {
    return new Promise((done, fail) => {
        const socket = createServer();

        socket.once("error", fail);
        socket.listen(0, "127.0.0.1", () => {
            const held = socket.address();

            if (held === null || typeof held === "string") {
                return fail(new Error("the socket did not report a port"));
            }

            socket.close(() => done(held.port));
        });
    });
}

async function answers(url: string, until: number, alive: () => boolean) {
    while (Date.now() < until) {
        if (!alive()) throw new Error("the host exited before the debugger came up");

        try {
            const answer = await fetch(url, { signal: AbortSignal.timeout(500) });
            if (answer.ok) return;
        } catch {
            // the listener is not up yet
        }

        await new Promise((done) => setTimeout(done, 50));
    }

    throw new Error(`the debugger did not answer on ${url}`);
}

function spread(grid: Grid) {
    return [grid[0] ?? 1, grid[1] ?? 1, grid[2] ?? 1].join(",");
}

export async function launch(options: Options): Promise<Session> {
    if (!existsSync(HOST)) build();

    const port = options.port ?? (await free());
    const url = `http://127.0.0.1:${port}`;

    const repeat = (options.launches ?? 1) > 1 ? ["--repeat", String(options.launches)] : [];

    const child: ChildProcess = spawn(
        HOST,
        [
            ...repeat,
            join(HERE, "kernels", options.kernel),
            options.entry,
            spread(options.global),
            spread(options.local),
            ...(options.arguments ?? []),
        ],
        {
            cwd: ROOT,
            env: {
                ...process.env,
                [LIBRARY_PATH]: DRIVER,
                VODD_DEBUG: `127.0.0.1:${port}`,
                VODD_CHECK: options.checks ?? "all",
                VODD_MAX_ERRORS: String(options.maxErrors ?? 8),
                ...options.environment,
            },
        },
    );

    let said = "";
    let live = true;

    child.stderr?.on("data", (chunk) => (said += chunk.toString()));
    child.stdout?.on("data", (chunk) => (said += chunk.toString()));
    child.once("exit", () => (live = false));

    const session: Session = {
        url,
        port,
        output: () => said,
        running: () => live,
        stop: () => {
            if (live) child.kill("SIGKILL");
        },
    };

    try {
        await answers(url, Date.now() + READY, () => live);
    } catch (error) {
        session.stop();
        throw new Error(`${error}\n${said}`);
    }

    return session;
}
