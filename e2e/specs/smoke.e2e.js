// End-to-end smokes against the real built app: real Svelte UI ↔ real Rust
// backend ↔ real config.txt writes (in the throwaway data dir). One app session
// runs the whole file, so tests re-apply what they depend on rather than
// assuming a clean slate.
import { browser, $, $$, expect } from "@wdio/globals";
import { readConfig } from "../helpers/seed.js";

const BYPASS = 'button[title^="Drop the EQ filters"]';

const rows = () => $$(".presets li:not(.empty)");

async function presetNames() {
  // WDIO's $$ .map() is async and resolves the callbacks itself — don't wrap it
  // in Promise.all (that would try to iterate a Promise).
  const texts = await $$(".presets li:not(.empty) .name").map((el) => el.getText());
  return texts.map((t) => t.trim());
}

async function rowFor(name) {
  for (const li of await rows()) {
    const text = (await (await li.$(".name")).getText()).trim();
    if (text === name) return li;
  }
  throw new Error(`preset row not found: ${name}`);
}

async function apply(name) {
  await (await (await rowFor(name)).$(".name")).click();
}

describe("fastpeq E2E smokes", () => {
  it("launches and lists the seeded presets", async () => {
    await browser.waitUntil(async () => (await rows()).length >= 3, {
      timeout: 20000,
      timeoutMsg: "preset list never populated",
    });
    const names = await presetNames();
    expect(names).toContain("BassBoost");
    expect(names).toContain("Vocal");
    expect(names).toContain("Studio");
  });

  it("applies a preset: marks it active and writes its filters to config.txt", async () => {
    await apply("BassBoost");

    await browser.waitUntil(
      async () => ((await (await rowFor("BassBoost")).getAttribute("class")) || "").includes("active"),
      { timeout: 10000, timeoutMsg: "preset never became active in the list" },
    );

    const cfg = readConfig();
    expect(cfg).toContain("Preamp");
    expect(cfg).toContain("Filter");
  });

  it("bypass round-trip drops then restores the filters", async () => {
    await apply("BassBoost");
    await browser.waitUntil(() => readConfig().includes("Filter"), { timeout: 10000 });

    // Bypass: filters dropped, preamp kept.
    await $(BYPASS).click();
    await browser.waitUntil(() => !readConfig().includes("Filter"), {
      timeout: 10000,
      timeoutMsg: "filters were not dropped on bypass",
    });
    expect(readConfig()).toContain("Preamp");

    // Un-bypass: the exact prior config (filters and all) comes back.
    await $(BYPASS).click();
    await browser.waitUntil(() => readConfig().includes("Filter"), {
      timeout: 10000,
      timeoutMsg: "filters were not restored on un-bypass",
    });
  });

  it("creates a new preset from scratch and adds it to the list", async () => {
    await $("button.new-btn").click();
    await (await $(".create input")).setValue("Tester");
    await (await $(".create-actions .primary")).click();

    await browser.waitUntil(async () => (await presetNames()).includes("Tester"), {
      timeout: 10000,
      timeoutMsg: "the new preset never appeared in the list",
    });
  });

  it("filters the list by device type via the icon dropdown", async () => {
    await $(".type-trigger").click();
    const menu = await $(".type-menu");
    await menu.waitForExist({ timeout: 5000 });

    for (const item of await menu.$$(".cat-menu-item")) {
      if ((await item.getText()).trim() === "IEM") {
        await item.click();
        break;
      }
    }

    // Only Vocal is tagged iem; the created (uncategorized) preset drops out.
    await browser.waitUntil(
      async () => {
        const names = await presetNames();
        return names.length === 1 && names[0] === "Vocal";
      },
      { timeout: 8000, timeoutMsg: "device-type filter did not narrow to Vocal" },
    );
  });
});
