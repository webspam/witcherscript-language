# WitcherScript Parser Prototype

Small Rust CLI for parsing and validating WitcherScript (`.ws`) files with Tree-sitter.

This is a syntax validator and parse-tree inspection tool. It is not a formatter, and it
does not build a lossy semantic AST.

## Usage

From this directory:

```powershell
cargo run -- ..\src\LightRewrite.ws
cargo run -- ..\src ..\debug
cargo run -- ..\debug\editor\LRDebug_ToastOneLiner.ws --dump-tree
```

If Cargo is not on `PATH` in PowerShell, use:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -- ..\src ..\debug
```

The command accepts one or more file or directory paths. Directory inputs are searched
recursively for `.ws` files.

Exit codes:

- `0`: all parsed files have no diagnostics.
- `1`: one or more files parsed with syntax or validation diagnostics.
- `2`: CLI, IO, setup, or parser initialisation error.

## Diagnostics

Diagnostics include the file path, start line/column, end line/column, byte range, node
kind, and a source-line snippet when available.

Current validation rules:

- Local `var` declarations must precede executable statements within each function block.
  Blank lines, comments, and bare semicolons do not count as executable statements.

`--dump-tree` prints a concrete syntax tree with node kinds plus line/column and byte
ranges. This keeps comments, token positions, and concrete structure visible for future
formatter experiments without introducing a lossy AST layer.

## Current Validation Result

Validated against the local Light Rewrite corpus:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -- ..\src ..\debug --max-diagnostics 5
```

Result: all 32 `.ws` files under `src/` and `debug/` parsed cleanly with
`tree-sitter-witcherscript` tag `v0.13.0` from
`https://github.com/webspam/tree-sitter-witcherscript`.

No syntax or local variable ordering failures were found in the local corpus during this
pass.

## Caveats

- This tool reports Tree-sitter parse errors plus a small set of explicit validation rules.
  It does not reject every construct that the WitcherScript compiler or this repo's style
  rules may reject.
- The current grammar accepts ternary expressions, even though this project treats
  ternaries as invalid WitcherScript. That is deliberately documented rather than patched
  in this prototype.
- The grammar dependency is pinned to the `webspam` fork so future grammar fixes can be
  made outside this repo and consumed by retargeting the Cargo dependency.
