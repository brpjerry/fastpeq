// End-to-end smokes against the real built app: real Svelte UI ↔ real Rust
// backend ↔ real config.txt writes (in the throwaway data dir). One app session
// runs the whole file, so tests re-apply what they depend on rather than
// assuming a clean slate.
import { browser, $, $$, expect } from "@wdio/globals";
import {
  readConfig,
  readPreset,
  presetExists,
  readVariant,
  readVariants,
  variantExists,
} from "../helpers/seed.js";

const BYPASS = 'button[title^="Drop the EQ filters"]';

// The Bypass button is disabled while an apply is in flight, and a click on a
// disabled button does nothing — so wait for it to be live before clicking.
async function clickBypass() {
  const bypass = await $(BYPASS);
  await bypass.waitForEnabled({ timeout: 10000 });
  await bypass.click();
}

const rows = () => $$(".presets li:not(.empty)");

// Selectors unique to the editor (the presets panel has none of these), so they
// can be queried globally to mean "the open editor".
const GRAPH = ".graph-wrap"; // present once a preset is selected
const SAVE = ".actions .primary"; // the editor's Save / Saved button
const bandRows = () => $$(".band");
// `$` is strict since WebdriverIO 10 (it throws when a selector matches more
// than one element), and a preset has several bands — so "the first band" is
// spelled out as the first of `$$`.
const firstBand = () => $$(".band")[0];
const firstBandTok = () => $$(".band .ts-btn .tok")[0];

// A preset row's name: the `.name` button's own text, without the "v2" badge
// (`.ver`) it gains once the preset has saved history — getText() would run
// the two together as "BassBoostv2".
const nameOf = (button) =>
  browser.execute(
    (el) =>
      [...el.childNodes]
        .filter((n) => n.nodeType === Node.TEXT_NODE)
        .map((n) => n.textContent)
        .join("")
        .trim(),
    button,
  );

async function presetNames() {
  // WDIO's $$ .map() is async and resolves the callbacks itself — don't wrap it
  // in Promise.all (that would try to iterate a Promise).
  return $$(".presets li:not(.empty) .name").map(nameOf);
}

async function rowFor(name) {
  for (const li of await rows()) {
    if ((await nameOf(await li.$(".name"))) === name) return li;
  }
  throw new Error(`preset row not found: ${name}`);
}

// Click a preset row and wait for the apply to land. The panel is `busy` (its
// buttons disabled) until the backend has applied the preset, so a click fired
// meanwhile would be lost on a disabled button.
async function apply(name) {
  await (await (await rowFor(name)).$(".name")).click();
  await browser.waitUntil(
    async () => ((await (await rowFor(name)).getAttribute("class")) || "").includes("active"),
    { timeout: 10000, timeoutMsg: `${name} never became active` },
  );
  const newBtn = await $("button.new-btn");
  if (await newBtn.isExisting()) await newBtn.waitForEnabled({ timeout: 10000 });
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
  const newBtn = await $("button.new-btn");
  await newBtn.waitForEnabled({ timeout: 10000 }); // a previous action may still be busy
  await newBtn.click();
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
      async () =>
        ((await (await rowFor("BassBoost")).getAttribute("class")) || "").includes("active"),
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
    await clickBypass();
    await browser.waitUntil(() => !readConfig().includes("Filter"), {
      timeout: 10000,
      timeoutMsg: "filters were not dropped on bypass",
    });
    expect(readConfig()).toContain("Preamp");

    // Un-bypass: the exact prior config (filters and all) comes back.
    await clickBypass();
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
    expect(await (await $(".panel-head h2[title]")).getText()).toBe("BassBoost");
  });

  it("changes a band's filter type and persists it on save", async () => {
    await apply("BassBoost"); // re-load so a re-run starts from the seeded PK bands
    await $(GRAPH).waitForExist({ timeout: 10000 });

    await browser.waitUntil(async () => (await (await firstBandTok()).getText()).trim() === "PK", {
      timeout: 10000,
      timeoutMsg: "first band did not load as a Peak filter",
    });

    // Switch PK → LSC. This is the exact path that threw before the fix, because
    // the inline FilterList was missing onChangeKind.
    await pickBandType(await firstBand(), "LSC");
    await browser.waitUntil(async () => (await (await firstBandTok()).getText()).trim() === "LSC", {
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

    await (await $(".rename-input")).waitForExist({ timeout: 5000 });
    // The box auto-focuses and selects its text. Type into the focused element
    // rather than setValue(): setValue()'s clear step blurs the box, and its
    // onblur commits the (unchanged) name and unmounts it before keys land.
    await browser.keys(["Control", "a"]); // select existing text
    await browser.keys("Renamed"); // replace it
    await browser.keys(["Enter"]); // commit

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

// ── Tier 3: search + editor tooling ──────────────────────────────────────────
describe("search", () => {
  before(showAll);

  it("narrows the list and Enter applies the top match", async () => {
    const search = await $(".search");
    await search.setValue("Stud");
    await browser.waitUntil(
      async () => {
        const names = await presetNames();
        return names.length === 1 && names[0] === "Studio";
      },
      { timeout: 8000, timeoutMsg: "search did not narrow to Studio" },
    );

    await search.click(); // keep focus in the box, then submit
    await browser.keys(["Enter"]);

    await browser.waitUntil(
      async () => ((await (await rowFor("Studio")).getAttribute("class")) || "").includes("active"),
      { timeout: 10000, timeoutMsg: "Enter did not apply the top match" },
    );
    expect(readConfig()).toContain("Fc 8000 Hz");
  });
});

describe("editor tooling: compare, undo/redo, auto preamp", () => {
  before(showAll);

  it("A/B compare arms only after an edit and toggles back to it", async () => {
    await apply("Vocal");
    await $(GRAPH).waitForExist({ timeout: 10000 });
    const compare = await $(".compare-btn");
    expect(await compare.isEnabled()).toBe(false); // nothing unsaved to compare yet

    await pickBandType(await firstBand(), "LSC"); // an unsaved edit
    await browser.waitUntil(async () => await compare.isEnabled(), {
      timeout: 8000,
      timeoutMsg: "compare stayed disabled after an edit",
    });

    await compare.click();
    await browser.waitUntil(async () => (await compare.getText()).includes("Comparing"), {
      timeout: 5000,
      timeoutMsg: "compare did not arm",
    });
    // While comparing, the editor auditions the saved version.
    expect(await (await $(".live")).getText()).toContain("saved");

    await compare.click();
    await browser.waitUntil(async () => (await compare.getText()).trim() === "Compare", {
      timeout: 5000,
      timeoutMsg: "compare did not return to the edit",
    });
  });

  it("undo reverts an edit and redo reapplies it", async () => {
    await apply("Studio");
    await $(GRAPH).waitForExist({ timeout: 10000 });
    await browser.waitUntil(async () => (await (await firstBandTok()).getText()).trim() === "PK", {
      timeout: 10000,
      timeoutMsg: "Studio band did not load as PK",
    });

    const undoBtn = await $(".undo-btn");
    const redoBtn = await $(".redo-btn");
    expect(await undoBtn.isEnabled()).toBe(false);

    await pickBandType(await firstBand(), "LSC");
    // History records the edit after a short debounce, enabling undo.
    await browser.waitUntil(async () => await undoBtn.isEnabled(), {
      timeout: 5000,
      timeoutMsg: "undo did not enable after an edit",
    });

    await undoBtn.click();
    await browser.waitUntil(async () => (await (await firstBandTok()).getText()).trim() === "PK", {
      timeout: 5000,
      timeoutMsg: "undo did not revert the type change",
    });

    await browser.waitUntil(async () => await redoBtn.isEnabled(), {
      timeout: 5000,
      timeoutMsg: "redo did not enable after undo",
    });
    await redoBtn.click();
    await browser.waitUntil(async () => (await (await firstBandTok()).getText()).trim() === "LSC", {
      timeout: 5000,
      timeoutMsg: "redo did not reapply the type change",
    });
  });

  it("Auto Preamp disables the manual preamp slider", async () => {
    await apply("BassBoost");
    await $(GRAPH).waitForExist({ timeout: 10000 });
    const slider = await $('.preamp input[type="range"]');
    const auto = await $(".pside .switch"); // the Auto toggle beside the preamp row

    // Normalize to Auto-off first (the setting persists in prefs.json).
    if (!(await slider.isEnabled())) {
      await auto.click();
      await browser.waitUntil(async () => await slider.isEnabled(), { timeout: 5000 });
    }
    expect(await slider.isEnabled()).toBe(true);

    await auto.click();
    await browser.waitUntil(async () => !(await slider.isEnabled()), {
      timeout: 5000,
      timeoutMsg: "Auto Preamp did not disable the manual slider",
    });

    // Leave it off so a re-run starts from a known state.
    await auto.click();
    await browser.waitUntil(async () => await slider.isEnabled(), {
      timeout: 5000,
      timeoutMsg: "toggling Auto off did not re-enable the slider",
    });
  });
});

// ── Sub-presets: pad / tip variants inside one preset ────────────────────────
describe("sub-presets (pad / tip variants)", () => {
  before(showAll);

  it("creates a pad variant of the selected preset and switches to it", async () => {
    // The create button's label follows the selected preset's device type.
    await apply("Vocal"); // iem
    const btn = await $("button.variant-btn");
    await btn.waitForEnabled({ timeout: 10000 });
    expect(await btn.getText()).toBe("+ New Tips");
    await apply("BassBoost"); // headphone
    await browser.waitUntil(async () => (await btn.getText()) === "+ New Pads", {
      timeout: 5000,
      timeoutMsg: "create button did not switch to + New Pads",
    });

    // An edit left unsaved is what a roll is for: it goes into the new pads.
    // (BassBoost has two seeded bands, Vocal one — so two rows means loaded.)
    await browser.waitUntil(async () => (await bandRows()).length === 2, {
      timeout: 10000,
      timeoutMsg: "BassBoost's bands did not load",
    });
    await pickBandType(await firstBand(), "HSC");
    await browser.waitUntil(async () => (await (await firstBandTok()).getText()).trim() === "HSC", {
      timeout: 5000,
      timeoutMsg: "type picker did not switch to HSC",
    });

    await btn.click();
    const input = await $(".create input");
    expect(await input.getAttribute("placeholder")).toBe("Pad name");
    await input.setValue("Dekoni");
    await (await $(".create-actions .primary")).click();

    // Both the seeded Default and the new variant exist, the new one is
    // selected (the sidecar is the last thing written), and the preset file
    // mirrors it (a copy of what it had).
    await browser.waitUntil(
      () =>
        variantExists("BassBoost", "Default") &&
        variantExists("BassBoost", "Dekoni") &&
        readVariants().BassBoost?.selected === "Dekoni",
      { timeout: 10000, timeoutMsg: "variant files / selection were not written" },
    );
    expect(readPreset("BassBoost")).toBe(readVariant("BassBoost", "Dekoni"));
    expect(readPreset("BassBoost")).toContain("Fc 60 Hz"); // BassBoost's own filters
    // The unsaved edit went into the new pads; Default kept what was saved.
    expect(readVariant("BassBoost", "Dekoni")).toContain("HSC");
    expect(readVariant("BassBoost", "Default")).not.toContain("HSC");
    await browser.waitUntil(async () => (await (await $(SAVE)).getText()).trim() === "Saved", {
      timeout: 10000,
      timeoutMsg: "the editor did not settle on the saved variant",
    });

    // The row grew an expand caret, and its list opened marking the new pads.
    const row = await rowFor("BassBoost");
    await browser.waitUntil(
      async () => (await (await row.$(".variant-item.sel")).getText()).trim() === "Dekoni pads",
      { timeout: 10000, timeoutMsg: "row did not show the selected variant" },
    );
    expect(await (await row.$(".variant-toggle")).getAttribute("aria-expanded")).toBe("true");
    // No extra list rows: variants live inside the preset's row.
    expect((await presetNames()).filter((n) => n === "BassBoost").length).toBe(1);
  });

  it("picks another variant from the row's list, and the preset keeps loading it", async () => {
    const row = await rowFor("BassBoost");
    const list = await row.$(".variants"); // still open from creating the pads
    await list.waitForExist({ timeout: 5000 });
    for (const item of await list.$$(".variant-item")) {
      if ((await item.getText()).trim() === "Default pads") {
        await item.click();
        break;
      }
    }
    // The preset file is copied first, then the sidecar records the selection.
    await browser.waitUntil(
      () =>
        readVariants().BassBoost?.selected === "Default" &&
        readPreset("BassBoost") === readVariant("BassBoost", "Default"),
      { timeout: 10000, timeoutMsg: "picking Default pads did not update the selection" },
    );
    await browser.waitUntil(
      async () => (await (await row.$(".variant-item.sel")).getText()).trim() === "Default pads",
      { timeout: 10000, timeoutMsg: "row did not show Default pads" },
    );

    // Clicking the preset itself loads it as-is: the selection sticks.
    await apply("Studio");
    await apply("BassBoost");
    await browser.waitUntil(() => readConfig().includes("fastpeq:preset=BassBoost"), {
      timeout: 10000,
      timeoutMsg: "BassBoost was not re-applied",
    });
    expect(readVariants().BassBoost?.selected).toBe("Default");
  });
});
