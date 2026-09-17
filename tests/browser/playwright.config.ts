import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  globalSetup: "./setup.ts",
  timeout: 120_000,
  expect: { timeout: 30_000 },
  fullyParallel: true,
  workers: process.env.CI ? 2 : 4,
  reporter: [["list"]],
  use: {
    browserName: "chromium",
    viewport: { width: 1600, height: 1000 },
    trace: "off",
  },
});
