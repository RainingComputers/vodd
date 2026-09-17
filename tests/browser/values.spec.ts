import { Debugger, expect, test } from "./debugger";

const MIX = {
  kernel: "mix.cl",
  entry: "mix",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:1024", "local:256"],
};

const NESTED = {
  kernel: "nested.cl",
  entry: "nested",
  global: [256] as [number],
  local: [64] as [number],
  arguments: ["buffer:1024", "buffer:1024"],
};

async function stopped(page, vodd, options, line: number) {
  const session = await vodd(options);
  const held = await Debugger.open(page, session);

  await held.gutter(line).click();
  await held.press("resume");
  await expect(held.status.locator(".state")).toHaveText("paused");

  return held;
}

test("every local in scope is named with its value and its type", async ({ page, vodd }) => {
  const held = await stopped(page, vodd, MIX, 13);

  await expect(held.values).toHaveCount(8);

  await expect(held.named("scale")).toContainText("2.5");
  await expect(held.named("scale")).toContainText("float");

  await expect(held.named("lid")).toContainText("int");
  await expect(held.named("packed")).toContainText("float4");

  await expect(held.refusals).toHaveCount(0);
});

test("a vector decodes to its components, not its bit patterns", async ({ page, vodd }) => {
  const held = await stopped(page, vodd, MIX, 13);

  await expect(held.named("packed")).toContainText("(2.5, 3.5, 2.0, 3.0)");
  await expect(held.refusals).toHaveCount(0);
});

test("values are classified by how they vary across the lanes", async ({ page, vodd }) => {
  const held = await stopped(page, vodd, MIX, 13);

  await expect(held.named("in").locator(".pill")).toHaveText("shared");
  await expect(held.named("tile").locator(".pill")).toHaveText("shared");
  await expect(held.named("lid").locator(".pill")).toHaveText("affine");
  await expect(held.named("gid").locator(".pill")).toHaveText("affine");
  await expect(held.named("scale").locator(".pill")).toHaveText("uniform");

  await expect(held.named("lid")).toContainText("0 .. 63, 64 distinct");
  await expect(held.refusals).toHaveCount(0);
});

test("a pointer shows the memory region it points into", async ({ page, vodd }) => {
  const held = await stopped(page, vodd, MIX, 13);

  await expect(held.named("in")).toContainText("global 0x");
  await expect(held.named("tile")).toContainText("local 0x");
  await expect(held.named("in")).toContainText("float *");

  await expect(held.refusals).toHaveCount(0);
});

test("the values follow the frame the lanes are actually in", async ({ page, vodd }) => {
  const held = await stopped(page, vodd, NESTED, 3);

  await expect(held.frames).toHaveCount(3);
  await expect(held.values).toHaveCount(2);
  await expect(held.named("value")).toContainText("int");
  await expect(held.named("held")).toContainText("int");

  await expect(held.named("in")).toHaveCount(0);
  await expect(held.named("gid")).toHaveCount(0);

  await expect(held.refusals).toHaveCount(0);
});
