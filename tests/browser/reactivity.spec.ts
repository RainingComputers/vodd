import { Debugger, expect, test } from "./debugger";

const HISTOGRAM = {
  kernel: "histogram.cl",
  entry: "histogram",
  global: [4096] as [number],
  local: [64] as [number],
  arguments: ["buffer:16384", "buffer:64", "local:256"],
};

async function finished(page, vodd, options = HISTOGRAM) {
  const session = await vodd(options);
  const held = await Debugger.open(page, session);

  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("finished");

  return { session, held };
}

async function watching(page, vodd, line: number, options = HISTOGRAM) {
  const session = await vodd(options);
  const held = await Debugger.open(page, session);

  await held.gutter(line).click();
  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("paused");

  return { session, held };
}

test("choosing a work group swaps in its work items", async ({ page, vodd }) => {
  const { held } = await finished(page, vodd);

  await expect(held.groups).toHaveCount(64);

  const chosen = held.groups.nth(63);
  await chosen.click();

  await expect(chosen).toHaveClass(/\bon\b/);
  await expect(held.fragment("items")).toContainText("Work items");
  await expect(held.items).toHaveCount(64);
  await expect(held.refusals).toHaveCount(0);
});

test("a group nobody is watching says so instead of showing another group", async ({
  page,
  vodd,
}) => {
  const { held } = await watching(page, vodd, 12);

  await expect(held.paths).toHaveCount(1);
  await expect(held.current).toHaveCount(1);

  await held.pick(40);

  await expect(held.paths).toHaveCount(0);
  await expect(held.fragment("paths")).toContainText("has not started");
  await expect(held.fragment("side")).toContainText("has not started");
  await expect(held.current).toHaveCount(0);

  await expect(held.refusals).toHaveCount(0);
});

test("choosing a work item marks its lane and its path", async ({ page, vodd }) => {
  const { held } = await watching(page, vodd, 12);

  const lane = held.items.nth(9);
  await lane.click();

  await expect(lane).toHaveClass(/\bon\b/);
  await expect(held.paths.filter({ hasText: "P" }).first()).toBeVisible();
  await expect(held.refusals).toHaveCount(0);
});

test("the execution path tallies every work item and marks its line", async ({ page, vodd }) => {
  const { held } = await watching(page, vodd, 12);

  await expect(held.paths).toHaveCount(1);
  await expect(held.paths.first()).toContainText("64");
  await expect(held.paths.first()).toHaveAttribute("aria-current", "true");

  await expect(held.current).toHaveCount(1);
  await expect(held.fragment("paths")).toContainText("1 path over 64");
  await expect(held.refusals).toHaveCount(0);
});

test("choosing a diagnostic opens its detail rows", async ({ page, vodd }) => {
  const { held } = await finished(page, vodd);

  await expect(held.findings.first()).toContainText("data race");

  const third = held.findings.nth(2);
  await third.click();

  await expect(third).toHaveAttribute("aria-current", "true");

  const detail = held.fragment("dock").locator(".aside");

  await expect(detail).toHaveCount(1);
  await expect(detail).toContainText("first item");
  await expect(detail).toContainText("second item");
  await expect(held.refusals).toHaveCount(0);
});

test("two launches get two tabs and the strip switches panes", async ({ page, vodd }) => {
  const session = await vodd({ ...HISTOGRAM, launches: 2 });
  const first = await Debugger.open(page, session);
  const second = first.on(1);

  await expect(first.tabs).toHaveCount(2);
  await expect(first.pane).toBeVisible();
  await expect(second.pane).toBeHidden();

  await first.press("resume");
  await expect(first.status.locator(".state")).toHaveText("finished");

  await first.press("resume");

  await first.tabs.nth(1).click();

  await expect(second.pane).toBeVisible();
  await expect(first.pane).toBeHidden();
  await expect(second.status.locator(".state")).toHaveText("paused");
  await expect.poll(() => second.enabled("resume")).toBe(true);

  await second.press("resume");
  await expect(second.status.locator(".state")).toHaveText("finished");

  await expect(second.refusals).toHaveCount(0);
});
