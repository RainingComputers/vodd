import { Debugger, expect, test } from "./debugger";

const BRANCH = {
  kernel: "branch.cl",
  entry: "branch",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:1024"],
};

const TILE = {
  kernel: "tile.cl",
  entry: "tile",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:1024", "local:256"],
};

async function until(held: Debugger, line: number, limit = 40) {
  for (let round = 0; round < limit; round++) {
    if ((await held.line) === line) return;
    await held.press("step");
    await expect(held.status.locator(".state")).toHaveText("paused");
  }

  throw new Error(`no lane reached line ${line} within ${limit} steps`);
}

test("stepping by item advances the lanes one source line at a time", async ({ page, vodd }) => {
  const session = await vodd(BRANCH);
  const held = await Debugger.open(page, session);

  const walked = new Set<string>();

  walked.add(await held.site);

  for (let round = 0; round < 6; round++) {
    const before = await held.site;

    await held.press("step");
    await expect.poll(() => held.site).not.toBe(before);
    await expect(held.status.locator(".state")).toHaveText("paused");

    walked.add(await held.site);
  }

  expect(walked.size).toBe(7);
  expect(await held.progress).toBe(1);

  await expect(held.lanes).toHaveCount(64);
  await expect(held.refusals).toHaveCount(0);
});

test("a branch splits the lanes into two coloured paths", async ({ page, vodd }) => {
  const session = await vodd(BRANCH);
  const held = await Debugger.open(page, session);

  await expect.poll(async () => {
    await held.press("step");
    return held.paths.count();
  }).toBe(2);

  const rows = held.paths;

  await expect(rows).toHaveCount(2);

  const chips = await rows.locator(".chip").evaluateAll((found) =>
    found.map((one) => getComputedStyle(one).backgroundColor),
  );

  expect(new Set(chips).size).toBe(2);

  const tallies = await rows
    .locator(".tally")
    .evaluateAll((found) => found.map((one) => Number(one.textContent!.trim())));

  expect(tallies.reduce((a, b) => a + b, 0)).toBe(64);
  expect(tallies).toContain(16);
  expect(tallies).toContain(48);

  await expect(held.refusals).toHaveCount(0);
});

test("lanes park together at a barrier and are released together", async ({ page, vodd }) => {
  const session = await vodd(TILE);
  const held = await Debugger.open(page, session);


  const parked = held.fragment("items").locator(".cell.parked");

  await expect
    .poll(
      async () => {
        await held.press("step");
        return parked.count();
      },
      { message: "every lane has to reach the barrier" },
    )
    .toBe(64);

  await held.press("step");
  await expect(parked).toHaveCount(0);
  await expect(held.refusals).toHaveCount(0);
});
