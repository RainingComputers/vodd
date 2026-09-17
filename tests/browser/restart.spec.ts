import { Debugger, expect, test } from "./debugger";

const SAXPY = {
  kernel: "saxpy.cl",
  entry: "saxpy",
  global: [64] as [number],
  local: [16] as [number],
  arguments: ["buffer:256", "buffer:256"],
};

test("killing the program says so and restarting it revives the page", async ({ page, vodd }) => {
  const first = await vodd({ ...SAXPY });
  const held = await Debugger.open(page, first);

  const banner = page.locator(".link");

  await expect(held.status.locator(".state")).toHaveText("paused");
  await expect(banner).toBeHidden();

  first.stop();

  await expect(banner).toBeVisible();
  await expect(banner).toContainText("Debugger disconnected");

  const again = await vodd({ ...SAXPY, port: first.port });

  await expect(banner).toBeHidden();
  await expect(held.status.locator(".state")).toHaveText("paused");

  await held.press("resume");

  await expect(held.status.locator(".state")).toHaveText("finished");
  await expect(held.refusals).toHaveCount(0);
  expect(again.running()).toBe(true);
});
