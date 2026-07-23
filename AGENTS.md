# fastpeq agent instructions

## Versioning

When a task requires a version bump, run the repository helper instead of
editing version strings by hand:

```sh
npm run bump-version -- <major.minor.patch>
```

For example, `npm run bump-version -- 0.9.0` updates the frontend, Tauri, and
Cargo application-version records, including the relevant lockfile records. Do
not use `npm version`, and do not modify dependency versions in lockfiles while
doing an application version bump.

## Installers

`src-tauri/main.wxs` is a **vendored copy** of tauri-bundler's stock WiX
template, carrying exactly one deviation (the Start Menu shortcut omits
`Icon="ProductIcon"`, so taskbar pins survive an upgrade — see the file header
and the README). Do not hand-edit it for unrelated reasons. When the Tauri
toolchain is upgraded, fetch the stock template for the new tauri-bundler
version, diff it against this copy, and re-apply the single deviation on top of
the new stock file.
