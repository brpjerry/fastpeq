// End-to-end smokes against the real built app: real Svelte UI ↔ real Rust
// backend ↔ real config.txt writes (in the throwaway data dir). One app session
// runs the whole file, so tests re-apply what they depend on rather than
// assuming a clean slate.
import { browser, $, $$, expect } from "@wdio/globals";
import { readConfig, readPreset, presetExists } from "../helpers/seed.js";

const BYPASS = 'button[title^="Drop the EQ filters"]';

const rows = () => $$(".presets li:not(.empty)");

// Selectors unique to the editor (the presets panel has none of these), so they
// can be queried globally to mean "the open editor".
const GRAPH = ".graph-wrap"; // present once a preset is selected
const SAVE = ".actions .primary"; // the editor's Save / Saved button
const bandRows = () => $$(".band");

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

// Reset the list view: clear the search box and set the type filter back to
// "All types". An earlier test (the device-type filter) leaves the list narrowed
// and the search may carry a query, so feature tests start from the full list.
async function showAll() {
  const search = await $(".search");
  if (await search.isExisting()) await search.setValue("");
  await $(".type-trigger").click();
  const menu = await $(".type-menu");
  await menu.waitForExist({ timeout: 5000 });
  for (const item of await menu.$$(".cat-menu-item")) {
    if ((await item.getText()).trim() === "All types") {
      await item.click();
      break;
    }
  }
  await menu.waitForExist({ timeout: 5000, reverse: true });
}

// Select a band's filter-type token (e.g. "PK", "LSC") from its type dropdown.
async function pickBandType(band, token) {
  await (await band.$(".ts-btn")).click();
  const menu = await $(".ts-menu");
  await menu.waitForExist({ timeout: 5000 });
  for (const item of await menu.$$(".ts-item")) {
    if ((await (await item.$(".tok")).getText()).trim() === token) {
      await item.click();
      return;
    }
  }
  throw new Error(`filter type not offered: ${token}`);
}

// Create a new (empty) preset via the "+ New preset" → "From scratch" flow.
async function createPreset(name) {
  await $("button.new-btn").click();
  await (await $(".create input")).setValue(name);
  await (await $(".create-actions .primary")).click();
  await browser.waitUntil(async () => (await presetNames()).includes(name), {
    timeout: 10000,
    timeoutMsg: `preset "${name}" was not created`,
  });
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

// ── Tier 1: the editor ───────────────────────────────────────────────────────
// The smokes above never open the editor, so its whole surface — the part the
// file-split refactor reorganized — was untested. This tier exercises the edit
// path that regressed (changing a band's filter type) plus the save round-trip.
describe("editor: edit a band and save", () => {
  before(showAll);

  it("opens the editor with the preset's bands when one is selected", async () => {
    await apply("BassBoost");
    await $(GRAPH).waitForExist({ timeout: 10000, timeoutMsg: "editor never opened" });
    // BassBoost seeds two filters → two band rows.
    await browser.waitUntil(async () => (await bandRows()).length === 2, {
      timeout: 10000,
      timeoutMsg: "editor did not show BassBoost's two bands",
    });
    // The editor heading carries a title attribute; the presets heading doesn't.
    expect(await (await $('.panel-head h2[title]')).getText()).toBe("BassBoost");
  });

  it("changes a band's filter type and persists it on save", async () => {
    await apply("BassBoost"); // re-load so a re-run starts from the seeded PK bands
    await $(GRAPH).waitForExist({ timeout: 10000 });

    const firstTok = () => $(".band .ts-btn .tok");
    await browser.waitUntil(async () => (await (await firstTok()).getText()).trim() === "PK", {
      timeout: 10000,
      timeoutMsg: "first band did not load as a Peak filter",
    });

    // Switch PK → LSC. This is the exact path that threw before the fix, because
    // the inline FilterList was missing onChangeKind.
    await pickBandType(await $(".band"), "LSC");
    await browser.waitUntil(async () => (await (await firstTok()).getText()).trim() === "LSC", {
      timeout: 5000,
      timeoutMsg: "type picker did not switch to LSC (onChangeKind regression?)",
    });

    // Live edits flow straight to config.txt; the new token should land there.
    await browser.waitUntil(() => readConfig().includes("LSC"), {
      timeout: 10000,
      timeoutMsg: "type change was not applied to config.txt",
    });

    // Save writes the preset file and flips the button to "Saved".
    const save = await $(SAVE);
    await save.click();
    await browser.waitUntil(async () => (await save.getText()).trim() === "Saved", {
      timeout: 10000,
      timeoutMsg: "save button never settled to Saved",
    });
    expect(readPreset("BassBoost")).toContain("LSC");
  });
});

// ── Tier 2: tone overlay + preset lifecycle ──────────────────────────────────
describe("tone overlay", () => {
  before(showAll);

  it("a tone knob layers a filter over the active preset, and Reset clears it", async () => {
    await apply("BassBoost"); // tone is layered over whatever's active
    const bass = await $('[role="slider"][aria-label="Bass"]');
    await bass.waitForExist({ timeout: 10000 });

    // Drive the knob up via its keyboard a11y path (ArrowUp = +one step).
    await browser.execute((el) => el.focus(), bass);
    for (let i = 0; i < 4; i++) await browser.keys(["ArrowUp"]);

    await browser.waitUntil(async () => Number(await bass.getAttribute("aria-valuenow")) > 0, {
      timeout: 5000,
      timeoutMsg: "bass knob did not move with the keyboard",
    });
    // The bass tone is a low shelf at 105 Hz — a frequency no seeded preset uses.
    await browser.waitUntil(() => readConfig().includes("Fc 105 Hz"), {
      timeout: 10000,
      timeoutMsg: "tone overlay was not written to config.txt",
    });

    await (await $(".tone-reset")).click();
    await browser.waitUntil(() => !readConfig().includes("Fc 105 Hz"), {
      timeout: 10000,
      timeoutMsg: "Reset did not clear the tone overlay",
    });
    expect(Number(await bass.getAttribute("aria-valuenow"))).toBe(0);
  });
});

describe("preset lifecycle: rename, delete, capture", () => {
  before(showAll);

  it("renames a preset in the list and on disk", async () => {
    await createPreset("RenameMe");
    const row = await rowFor("RenameMe");
    await (await row.$('button[title="Rename"]')).click();

    const input = await $(".rename-input");
    await input.waitForExist({ timeout: 5000 });
    await input.setValue("Renamed");
    await browser.keys(["Enter"]);

    await browser.waitUntil(
      async () => {
        const names = await presetNames();
        return names.includes("Renamed") && !names.includes("RenameMe");
      },
      { timeout: 10000, timeoutMsg: "rename did not update the list" },
    );
    expect(presetExists("Renamed")).toBe(true);
    expect(presetExists("RenameMe")).toBe(false);
  });

  it("deletes a preset from the list and disk", async () => {
    await createPreset("DeleteMe");
    const row = await rowFor("DeleteMe");
    await (await row.$(".danger.icon")).click();

    await browser.waitUntil(async () => !(await presetNames()).includes("DeleteMe"), {
      timeout: 10000,
      timeoutMsg: "delete did not remove the preset from the list",
    });
    expect(presetExists("DeleteMe")).toBe(false);
  });

  it("captures the current live config as a new preset", async () => {
    await apply("Studio"); // make Studio the live config we're about to capture
    await $("button.new-btn").click();
    await (await $(".create input")).setValue("Captured");
    await (await $(".create-actions .capture-btn")).click();

    await browser.waitUntil(async () => (await presetNames()).includes("Captured"), {
      timeout: 10000,
      timeoutMsg: "captured preset never appeared in the list",
    });
    // Studio's signature filter (8 kHz) should be in the captured file.
    expect(readPreset("Captured")).toContain("Fc 8000 Hz");
  });
});
