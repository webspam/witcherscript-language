# Releasing

How to cut a new version of `witcherscript-language`.

The package has `publish = false` in `Cargo.toml`, so nothing goes to crates.io.
Releases are cut as GitHub Releases with prebuilt Windows binaries attached.

## Versioning

Semantic versioning (`MAJOR.MINOR.PATCH`):

- **MAJOR** - incompatible CLI or LSP behaviour changes.
- **MINOR** - new diagnostics, LSP capabilities, or grammar support.
- **PATCH** - bug fixes and internal changes with no user-visible feature change.

## Steps

1. Pick the new version and edit `version` in `Cargo.toml`.
2. Regenerate `Cargo.lock` so the `witcherscript-language` entry matches:

   ```
   just build
   ```

3. Verify the build is clean:

   ```
   just precommit
   ```

4. Stage only `Cargo.toml` and `Cargo.lock`, and commit:

   ```
   Version X.Y.Z
   ```

   Keep the bump in its own commit - no unrelated changes.

5. Push the bump to `master` (via PR or direct push, per repo policy).

6. Trigger the `Create Release` workflow ([.github/workflows/release.yml](.github/workflows/release.yml))
   from the Actions tab:

   - **tag** - `vX.Y.Z` (must match the version in `Cargo.toml`)
   - **title** - leave blank to use the tag
   - **summary** - optional prose to prepend above the auto-generated notes
   - **prerelease** - leave unchecked for a normal release

   The workflow builds release binaries on `windows-latest`, packages
   `witcherscript-check.exe` + `witcherscript-lsp.exe` (plus `README.md`) into
   `witcherscript-language-vX.Y.Z-windows-x64.zip`, creates the tag, and opens
   a **draft** GitHub Release with auto-generated notes from commits since the
   previous tag.

7. Review the draft release on GitHub, edit notes if needed, and publish.
