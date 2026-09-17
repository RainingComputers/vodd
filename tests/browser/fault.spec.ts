import { Debugger, expect, test } from "./debugger";

const STRAY = {
  kernel: "stray.cl",
  entry: "stray",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:64"],
};

const SAXPY = {
  kernel: "saxpy.cl",
  entry: "saxpy",
  global: [1024] as [number],
  local: [64] as [number],
  arguments: ["buffer:4096", "buffer:4096"],
};

test("a stray write is reported against its own line and lane", async ({ page, vodd }) => {
  const session = await vodd(STRAY);
  const held = await Debugger.open(page, session);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  expect(await held.problems).toBeGreaterThan(0);
  await expect(held.findings.first()).toContainText("past the end of its allocation");
  await expect(held.findings.first()).toContainText(":6:");

  await held.groups.nth(2).click();
  await expect(held.fragment("items").locator(".cell.marked")).toHaveCount(1);

  await expect(held.refusals).toHaveCount(0);
});

test("a clean kernel reports nothing and still fills every panel", async ({ page, vodd }) => {
  const session = await vodd(SAXPY);
  const held = await Debugger.open(page, session);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  expect(await held.problems).toBe(0);
  await expect(held.findings).toHaveCount(0);
  await expect(held.fragment("dock")).toContainText("No problems found.");

  await expect(held.groups).toHaveCount(16);
  await expect(held.items).toHaveCount(64);
  await expect(held.fragment("paths")).toContainText("no longer held");

  await expect(held.refusals).toHaveCount(0);
});

test("a held launch leaves only continue to press", async ({ page, vodd }) => {
  const session = await vodd(SAXPY);
  const held = await Debugger.open(page, session);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  const live = held.fragment("source").locator(".key:not([disabled])");

  await expect(live).toHaveCount(1);
  await expect(live).toHaveClass(/\bresume\b/);
  await expect(held.groups).toHaveCount(16);

  await held.press("resume");
  await expect.poll(() => session.running(), { timeout: 10_000 }).toBe(false);
});
