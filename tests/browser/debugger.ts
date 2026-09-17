import { Locator, Page, expect, test as base } from "@playwright/test";

import { Options, Session, launch } from "./vodd";

export { expect };

export type Opener = (options: Options) => Promise<Session>;

export const test = base.extend<{ vodd: Opener }>({
  vodd: async ({}, use) => {
    const held: Session[] = [];

    await use(async (options) => {
      const session = await launch(options);
      held.push(session);

      return session;
    });

    held.forEach((session) => session.stop());
  },
});

export type Control = "resume" | "step";

export class Debugger {
  constructor(
    readonly page: Page,
    readonly at = 0,
  ) {}

  static async open(page: Page, session: Session, at = 0) {
    await page.goto(session.url);

    const held = new Debugger(page, at);
    await held.pane.waitFor();

    return held;
  }

  on(at: number) {
    return new Debugger(this.page, at);
  }

  get pane(): Locator {
    return this.page.locator(`[data-launch="${this.at}"]`);
  }

  get status(): Locator {
    return this.page.locator(`#status-${this.at}`);
  }

  get tabs(): Locator {
    return this.page.locator("#tabs .tab");
  }

  fragment(name: string): Locator {
    return this.page.locator(`#${name}-${this.at}`);
  }

  key(control: Control): Locator {
    return this.fragment("source").locator(`.key.${control}`);
  }

  async press(control: Control) {
    await this.key(control).click();
  }

  get stance(): Promise<string> {
    return this.status.locator(".state").innerText();
  }

  get progress(): Promise<number> {
    return this.status
      .locator("span")
      .nth(3)
      .locator("b")
      .innerText()
      .then((text) => Number(text.split(" of ")[0].replaceAll(",", "")));
  }

  get problems(): Promise<number> {
    return this.status
      .locator(".pill")
      .innerText()
      .then((text) => Number(text.replaceAll(",", "")));
  }

  get groups(): Locator {
    return this.fragment("groups").locator(".cell");
  }

  get items(): Locator {
    return this.fragment("items").locator(".cell");
  }

  get findings(): Locator {
    return this.fragment("dock").locator(".findings li");
  }

  get paths(): Locator {
    return this.fragment("paths").locator(".tracks li");
  }

  get lanes(): Locator {
    return this.fragment("items").locator(".cell");
  }

  get site(): Promise<string> {
    return this.fragment("paths")
      .locator(".tracks li")
      .first()
      .locator(".grew")
      .getAttribute("title")
      .then((held) => (held ?? "").split("  ")[0]);
  }

  get line(): Promise<number> {
    return this.fragment("paths")
      .locator(".tracks li")
      .first()
      .locator(".grew")
      .getAttribute("title")
      .then((held) => Number((held ?? "").split(":")[1]));
  }

  async pick(group: number) {
    await this.groups.nth(group).click();
    await expect(this.groups.nth(group)).toHaveClass(/\bon\b/);
  }

  enabled(control: Control): Promise<boolean> {
    return this.key(control).isEnabled();
  }

  get values(): Locator {
    return this.fragment("side").locator(".vars li");
  }

  named(name: string): Locator {
    return this.values.filter({ has: this.page.locator(`.name:text-is("${name}")`) });
  }

  get frames(): Locator {
    return this.fragment("side").locator(".frames li");
  }

  get stops(): Locator {
    return this.fragment("source").locator(".ln.stop");
  }

  gutter(line: number): Locator {
    return this.fragment("source").getByTitle(`Toggle a breakpoint on line ${line}`);
  }

  get marked(): Locator {
    return this.fragment("source").locator(".ln.fault");
  }

  get current(): Locator {
    return this.fragment("source").locator(".ln.current");
  }

  get roots(): Locator {
    const names = ["tabs", "page"]
      .map((name) => `#${name}`)
      .concat(
        ["groups", "items", "source", "paths", "side", "dock", "dockbar", "status"].map(
          (name) => `#${name}-${this.at}`,
        ),
      );

    return this.page.locator(names.join(", "));
  }

  get refusals(): Locator {
    return this.pane.locator(".unavailable");
  }
}
