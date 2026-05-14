# Releasing

How to cut a new version of `witcherscript-parser`.

There is currently no published artifact — `publish = false` in `Cargo.toml`, so
nothing goes to crates.io. A "release" today just means a recorded version bump
so local builds and binaries carry a meaningful version number.

## Versioning

Semantic versioning (`MAJOR.MINOR.PATCH`):

- **MAJOR** — incompatible CLI or LSP behaviour changes.
- **MINOR** — new diagnostics, LSP capabilities, or grammar support.
- **PATCH** — bug fixes and internal changes with no user-visible feature change.

## Steps

1. Pick the new version and edit `version` in `Cargo.toml`.
2. Regenerate `Cargo.lock` so the `witcherscript-parser` entry matches:

   ```
   just build
   ```

3. Verify the build is clean:

   ```
   just test
   ```

4. Stage only `Cargo.toml` and `Cargo.lock`, and commit:

   ```
   Version X.Y.Z
   ```

   Keep the bump in its own commit — no unrelated changes.

## Public releases (not yet in use)

When the project starts producing public releases, extend the process with:

- A git tag `vX.Y.Z` on the version-bump commit, pushed to the remote.
- Release notes / a changelog covering what changed since the previous tag.
- Entire process to be run through GitHub Actions (in runner context, for clean from-source audit trail)

Until then, tags are unnecessary for local-only version bumps.
