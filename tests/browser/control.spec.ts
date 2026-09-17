import { Debugger, expect, test } from "./debugger";

const BRANCH = {
  kernel: "branch.cl",
  entry: "branch",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:1024"],
};

const HISTOGRAM = {
  kernel: "histogram.cl",
  entry: "histogram",
  global: [4096] as [number],
  local: [64] as [number],
  arguments: ["buffer:16384", "buffer:64", "local:256"],
};

test("a launch breaks on the first statement, past the prologue", async ({ page, vodd }) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  await expect(held.status.locator(".state")).toHaveText("paused");
  await expect(held.current).toHaveCount(1);
  await expect(held.current).toContainText("get_local_id");
  await expect(held.current).not.toContainText("kernel void");

  expect(await held.enabled("resume")).toBe(true);
  expect(await held.enabled("step")).toBe(true);

  await page.waitForTimeout(1200);
  await expect(held.status.locator(".state")).toHaveText("paused");
  expect(session.running()).toBe(true);
});

test("a step advances the watched group one source line", async ({ page, vodd }) => {
  const session = await vodd(BRANCH);
  const held = await Debugger.open(page, session);

  const walked = new Set<string>();

  walked.add(await held.site);

  for (let round = 0; round < 5; round++) {
    const before = await held.site;

    await held.press("step");
    await expect.poll(() => held.site).not.toBe(before);
    await expect(held.status.locator(".state")).toHaveText("paused");

    walked.add(await held.site);
  }

  expect(walked.size).toBe(6);
  expect(await held.progress).toBe(1);
  await expect(held.refusals).toHaveCount(0);
});

test("the controls only light for the group that is running", async ({ page, vodd }) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  expect(await held.enabled("step")).toBe(true);

  await held.pick(40);
  await expect.poll(() => held.enabled("step")).toBe(false);
  expect(await held.enabled("resume")).toBe(false);

  await held.pick(0);
  await expect.poll(() => held.enabled("step")).toBe(true);

  await expect(held.refusals).toHaveCount(0);
});

test("continuing runs on and the finished launch is held until released", async ({
  page,
  vodd,
}) => {
  const session = await vodd(HISTOGRAM);
  const held = await Debugger.open(page, session);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  expect(session.running()).toBe(true);
  expect(await held.enabled("step")).toBe(false);
  expect(await held.enabled("resume")).toBe(true);

  await held.press("resume");
  await expect.poll(() => session.running(), { timeout: 10_000 }).toBe(false);
});

const SAXPY = {
  kernel: "saxpy.cl",
  entry: "saxpy",
  global: [16] as [number],
  local: [4] as [number],
  arguments: ["buffer:64", "buffer:64"],
};

test("stepping to the end finishes the launch and clears the mark", async ({ page, vodd }) => {
  const session = await vodd(SAXPY);
  const held = await Debugger.open(page, session);

  await expect(held.status.locator(".state")).toHaveText("paused");

  for (let round = 0; round < 12; round += 1) {
    if ((await held.status.locator(".state").textContent())?.trim() === "finished") break;

    await held.press("step");
    await page.waitForTimeout(250);
  }

  await expect(held.status.locator(".state")).toHaveText("finished");
  await expect(held.current).toHaveCount(0);
  await expect(held.refusals).toHaveCount(0);
});

test("continuing through a breakpoint to the end clears the mark", async ({ page, vodd }) => {
  const session = await vodd(SAXPY);
  const held = await Debugger.open(page, session);

  await held.gutter(5).click();
  await page.waitForTimeout(250);

  for (let round = 0; round < 10; round += 1) {
    if ((await held.status.locator(".state").textContent())?.trim() === "finished") break;

    await held.press("resume");
    await page.waitForTimeout(400);
  }

  await expect(held.status.locator(".state")).toHaveText("finished");
  await expect(held.current).toHaveCount(0);
  await expect(held.refusals).toHaveCount(0);
});
