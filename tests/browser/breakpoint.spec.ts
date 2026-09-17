import { Debugger, expect, test } from "./debugger";

const BRANCH = {
  kernel: "branch.cl",
  entry: "branch",
  global: [1024] as [number],
  local: [64] as [number],
  arguments: ["buffer:4096", "buffer:4096"],
};

const NESTED = {
  kernel: "nested.cl",
  entry: "nested",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:1024"],
};

test("a breakpoint holds the launch where a lane reaches the line", async ({ page, vodd }) => {
  const session = await vodd(BRANCH);
  const held = await Debugger.open(page, session);

  await held.gutter(10).click();
  await expect(held.stops).toHaveCount(1);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("paused");

  await expect(held.paths).toHaveCount(2);
  await expect(held.fragment("paths")).toContainText("seen = seen - 1;");

  expect(await held.progress).toBe(1);
  await expect(held.refusals).toHaveCount(0);
});

test("clicking the gutter again clears the breakpoint", async ({ page, vodd }) => {
  const session = await vodd(BRANCH);
  const held = await Debugger.open(page, session);

  await held.gutter(10).click();
  await expect(held.stops).toHaveCount(1);

  await held.gutter(10).click();
  await expect(held.stops).toHaveCount(0);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");
  await expect(held.refusals).toHaveCount(0);
});

test("the call stack lists a frame for every call", async ({ page, vodd }) => {
  const session = await vodd(NESTED);
  const held = await Debugger.open(page, session);

  await held.gutter(3).click();
  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("paused");

  await expect(held.frames).toHaveCount(3);
  await expect(held.frames.nth(0)).toContainText("twice");
  await expect(held.frames.nth(1)).toContainText("shift");
  await expect(held.frames.nth(2)).toContainText("nested");

  await expect(held.frames.nth(0)).toContainText("source.cl:3");

  await expect(held.refusals).toHaveCount(0);
});
