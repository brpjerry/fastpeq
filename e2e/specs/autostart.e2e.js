// "Start with Windows" against the real app: the Settings switch -> the Rust
// command -> the actual HKCU Run key that Task Manager's Startup apps tab reads.
//
// Unlike the other smokes this touches machine state outside the throwaway data
// dir, so it saves whatever `fastpeq` entry the machine already has (an
// installed copy may legitimately be registered) and puts it back afterwards.
import { browser, $, $$, expect } from "@wdio/globals";
import { execFileSync } from "node:child_process";

const RUN_KEY = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`;
const APPROVED_KEY = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`;
const NAME = "fastpeq";

// `reg` exits non-zero when the value doesn't exist, which is the common case
// here — every helper treats that as "absent" rather than an error.
function regQuery(key) {
  try {
    return execFileSync("reg", ["query", key, "/v", NAME], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
  } catch {
    return null;
  }
}

/** The Run entry's command line, or null when it isn't registered. */
function runValue() {
  const out = regQuery(RUN_KEY);
  if (out === null) return null;
  const line = out.split(/\r?\n/).find((l) => l.trim().startsWith(NAME));
  // "    fastpeq    REG_SZ    C:\...\fastpeq.exe --minimized"
  return line.trim().split(/\s{2,}/).slice(2).join(" ");
}

/** The `REG_SZ`/`REG_BINARY` data of a value, verbatim, for save/restore. */
function rawValue(key) {
  const out = regQuery(key);
  if (out === null) return null;
  const line = out.split(/\r?\n/).find((l) => l.trim().startsWith(NAME));
  const [, type, ...rest] = line.trim().split(/\s{2,}/);
  return { type, data: rest.join(" ") };
}

function deleteValue(key) {
  try {
    execFileSync("reg", ["delete", key, "/v", NAME, "/f"], { stdio: "ignore" });
  } catch {
    // already absent
  }
}

function restoreValue(key, saved) {
  if (saved === null) {
    deleteValue(key);
    return;
  }
  execFileSync("reg", ["add", key, "/v", NAME, "/t", saved.type, "/d", saved.data, "/f"], {
    stdio: "ignore",
  });
}

// Open (or return to) the settings page. The gear exists in the DOM before
// Svelte has wired its handler, so a click can land on nothing — wait for the
// seeded list first, then confirm the panel actually opened.
async function openSettings() {
  await $(".gear").waitForExist({ timeout: 20000, timeoutMsg: "app never rendered" });
  await browser.waitUntil(async () => (await $$(".presets li:not(.empty)")).length > 0, {
    timeout: 20000,
    timeoutMsg: "preset list never populated",
  });
  await $(".gear").click();
  await $(".settings-page").waitForExist({ timeout: 10000, timeoutMsg: "settings never opened" });
}

// The label wrapping the switch: its <input> is visually hidden (0x0, opacity 0),
// so the label is what a user — and WebDriver — can actually click.
async function startupSwitch() {
  for (const label of await $$(".switch")) {
    if ((await label.getText()).includes("Start with Windows")) return label;
  }
  throw new Error("Start with Windows switch not found");
}

const isChecked = async () => (await (await startupSwitch()).$("input")).isSelected();

describe("start with Windows", () => {
  let savedRun = null;
  let savedApproved = null;

  before(async () => {
    savedRun = rawValue(RUN_KEY);
    savedApproved = rawValue(APPROVED_KEY);
    deleteValue(RUN_KEY);
    deleteValue(APPROVED_KEY);
    await openSettings();
  });

  after(() => {
    restoreValue(RUN_KEY, savedRun);
    restoreValue(APPROVED_KEY, savedApproved);
  });

  it("starts off, reflecting an unregistered machine", async () => {
    const label = await startupSwitch();
    await label.scrollIntoView();
    // The switch is disabled until the backend's registry read resolves.
    await browser.waitUntil(async () => (await label.$("input")).isEnabled(), {
      timeout: 5000,
      timeoutMsg: "switch never became enabled (registry read didn't resolve)",
    });
    expect(await isChecked()).toBe(false);
    expect(runValue()).toBe(null);
  });

  it("registers a Run entry pointing at the exe with --minimized", async () => {
    await (await startupSwitch()).click();
    await browser.waitUntil(() => runValue() !== null, {
      timeout: 5000,
      timeoutMsg: "no Run value appeared",
    });

    const value = runValue();
    expect(value).toContain("fastpeq.exe");
    expect(value.endsWith("--minimized")).toBe(true);
    expect(await isChecked()).toBe(true);
  });

  it("reads the state back from the registry when Settings is reopened", async () => {
    await $(".gear").click(); // leave settings
    await $(".settings-page").waitForExist({ timeout: 10000, reverse: true });
    await openSettings(); // and back
    await (await startupSwitch()).scrollIntoView();
    await browser.waitUntil(isChecked, {
      timeout: 5000,
      timeoutMsg: "switch did not read back as on",
    });
  });

  it("removes the entry when switched off", async () => {
    const label = await startupSwitch();
    await label.scrollIntoView();
    await label.click();
    await browser.waitUntil(() => runValue() === null, {
      timeout: 5000,
      timeoutMsg: "Run value was not removed",
    });
    expect(await isChecked()).toBe(false);
  });
});
