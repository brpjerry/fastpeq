#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];

if (process.argv.length !== 3 || !isSemver(version)) {
  console.error("Usage: node scripts/bump-version.mjs <semver>");
  process.exit(1);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const updates = new Map();

updateJson("package.json", (manifest) => {
  manifest.version = version;
});

updateJson("package-lock.json", (lockfile) => {
  lockfile.version = version;
  if (!lockfile.packages?.[""]) {
    throw new Error("package-lock.json is missing its root package record.");
  }
  lockfile.packages[""].version = version;
});

updateJson("src-tauri/tauri.conf.json", (config) => {
  config.version = version;
});

for (const manifest of [
  "src-tauri/Cargo.toml",
  "crates/fastpeq-core/Cargo.toml",
  "crates/fastpeq-hw/Cargo.toml",
]) {
  updateCargoManifest(manifest);
}

for (const packageName of ["fastpeq", "fastpeq-core", "fastpeq-hw"]) {
  updateCargoLock(packageName);
}

for (const [file, contents] of updates) {
  const path = resolve(root, file);
  if (readFileSync(path, "utf8") !== contents) {
    writeFileSync(path, contents);
  }
}

console.log(`Updated version to ${version} in ${updates.size} files.`);

function isSemver(value) {
  if (typeof value !== "string") return false;

  const identifier = "(?:0|[1-9]\\d*|\\d*[A-Za-z-][0-9A-Za-z-]*)";
  const prerelease = `(?:-${identifier}(?:\\.${identifier})*)?`;
  const build = "(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?";
  return new RegExp(
    `^(0|[1-9]\\d*)\\.(0|[1-9]\\d*)\\.(0|[1-9]\\d*)${prerelease}${build}$`,
  ).test(value);
}

function updateJson(file, update) {
  const original = read(file);
  const parsed = JSON.parse(original);
  update(parsed);
  updates.set(file, `${JSON.stringify(parsed, null, 2)}\n`);
}

function updateCargoManifest(file) {
  const original = read(file);
  const updated = replaceExactlyOnce(
    original,
    /(^\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m,
    `$1"${version}"`,
    file,
  );
  updates.set(file, updated);
}

function updateCargoLock(packageName) {
  const file = "Cargo.lock";
  const escapedName = packageName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(
    `(\\[\\[package\\]\\]\\r?\\nname = "${escapedName}"\\r?\\nversion = )"[^"]+"`,
  );
  const original = updates.get(file) ?? read(file);
  updates.set(file, replaceExactlyOnce(original, pattern, `$1"${version}"`, file));
}

function replaceExactlyOnce(contents, pattern, replacement, file) {
  const globalFlags = pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`;
  const matches = [...contents.matchAll(new RegExp(pattern.source, globalFlags))];
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one version entry in ${file}.`);
  }
  return contents.replace(pattern, replacement);
}

function read(file) {
  return readFileSync(resolve(root, file), "utf8");
}
