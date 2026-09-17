import { Debugger, expect, test } from "./debugger";

const HISTOGRAM = {
  kernel: "histogram.cl",
  entry: "histogram",
  global: [16384] as [number],
  local: [64] as [number],
  arguments: ["buffer:65536", "buffer:64", "local:256"],
};

test("a launch waits at the first line of the first group", async ({ page, vodd }) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  await expect(held.status.locator(".state")).toHaveText("paused");
  expect(await held.progress).toBe(1);

  await expect(held.current).toHaveCount(1);
  await expect(held.current).not.toContainText("kernel void");
  await expect(held.lanes).toHaveCount(64);

  await expect(held.status).toContainText("global 16384 x 1 x 1");
  await expect(held.status).toContainText("local 64 x 1 x 1");
  await expect(held.status).toContainText("group 1 of 256");

  await expect(held.fragment("source")).toContainText("kernel void histogram");
  await expect(held.refusals).toHaveCount(0);

  await page.waitForTimeout(1500);
  expect(await held.progress).toBe(1);
  expect(session.running()).toBe(true);
});

test("resuming fills the lattice and streams diagnostics", async ({ page, vodd }) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  expect(await held.problems).toBe(0);

  await held.press("resume");

  await expect(held.status.locator(".state")).toHaveText("finished");
  expect(await held.progress).toBe(256);

  await expect(held.groups).toHaveCount(256);
  await expect(held.fragment("groups").locator(".cell.done")).toHaveCount(256);
  await expect(held.items).toHaveCount(64);

  expect(await held.problems).toBeGreaterThan(0);
  await expect(held.findings.first()).toContainText("data race");
  await expect(held.refusals).toHaveCount(0);
});

test("every fragment keeps its morph target across an update", async ({ page, vodd }) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  await expect(held.roots).toHaveCount(10);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  await expect(held.roots).toHaveCount(10);
});

test("the page resyncs to the live state after a reload", async ({ page, vodd }) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  const found = await held.problems;

  await page.reload();
  await held.pane.waitFor();

  await expect(held.status.locator(".state")).toHaveText("finished");
  expect(await held.progress).toBe(256);
  expect(await held.problems).toBe(found);
});
