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
