---
description: Start witcherscript-lsp in TCP listen mode, capture stderr, and stop after N log lines
---

# watchlogs

Launch the LSP server in TCP listen mode so the user can connect their editor,
capture stderr to a log file, stream the first N lines, then kill the server and
report. Use this when debugging editor-driven LSP behaviour (initialize traffic,
indexing, completion/hover requests, etc.).

## Arguments

`$ARGUMENTS` — optional. Free-form. Parse for:

- A port number (default `9257`)
- A line count (default `10`)
- A scenario hint ("until I disconnect", "until completions stop", etc.) — if
  given, prefer a `Monitor` filter that ends on the described condition rather
  than a fixed line count

If `$ARGUMENTS` is empty, use defaults (port 9257, 10 lines).

## Steps

### 1. Start the server in background

Use the `just lsp-listen <PORT>` recipe (defined in [justfile](justfile)). It
builds, deletes any stale log, and runs the binary directly (not `cargo run`)
with stderr redirected to `target/lsp-tcp.log`:

```bash
just lsp-listen <PORT>
```

Use `Bash` with `run_in_background: true`. **Save the returned task id** — you
need it for `TaskStop` later.

### 2. Wait for the listening banner before telling the user to connect

Use `Monitor` with a short `until` loop, not a sleep. The recipe may emit a
build line first, so grep for the banner string rather than just non-empty
file. The banner is always `witcherscript-lsp: listening on tcp://127.0.0.1:<port> (waiting for client)`:

```bash
until grep -q "listening on tcp" target/lsp-tcp.log 2>/dev/null; do sleep 0.2; done
grep "listening on tcp" target/lsp-tcp.log
```

Once that fires, tell the user the port and that they can connect their editor.

### 3. Stream the first N stderr lines, then auto-exit

`Monitor` emits one notification per stdout line. Use `tail -f` piped through
`awk` so the monitor exits exactly when N lines have flowed:

```bash
tail -n +1 -f target/lsp-tcp.log | awk 'NR==1{print; next} {print; if (++c >= N) exit}'
```

(Replace `N` with the requested line count. The `NR==1` clause makes the very
first line print without counting toward the cap, so the listening banner is
included for free.)

### 4. Stop the server

When the monitor's stream ends, call `TaskStop` on the server task id from
step 1. Don't rely on the monitor to kill the server — it only reads the log.

### 5. Report

Read `target/lsp-tcp.log` if you need more than the streamed lines, then summarise:

- Bind/accept (did the client actually connect?)
- Initialize payload (workspace roots, gameDirectory, initializationOptions)
- Configuration round-trip (`workspace/configuration`, `files.exclude`, etc.)
- Indexing progress and counts
- Any anomalies — errors, warnings, unexpected reindex, slow `total_us`

## Gotchas

- **Logs go to stderr**, not stdout. tower-lsp owns stdout for JSON-RPC frames.
  Always redirect with `2>` (not `>` or `&>`).
- **ANSI colour escapes** (`[2m`, `[32m`, …) are written to the log file even
  when stderr is redirected. If they make the log hard to read, strip them
  with `sed 's/\x1b\[[0-9;]*m//g' target/lsp-tcp.log` or fix `with_ansi(false)` in
  [src/bin/witcherscript-lsp/main.rs](src/bin/witcherscript-lsp/main.rs).
- **Default log filter when `--listen` is set and `RUST_LOG` is unset** is
  `warn,witcherscript_lsp=trace,witcherscript_parser=trace`. To override, set
  `RUST_LOG` before launching (e.g. `RUST_LOG=debug` for less noise).
- **Loopback only.** The server binds `127.0.0.1` — clients on the LAN cannot
  reach it.
- **Single client.** The server accepts one connection and serves it until
  disconnect; relaunch for a second session.
- **Don't `cargo run`** in background — its build output interleaves with LSP
  stderr and breaks the line-count cap.
- **Don't pre-emptively `TaskStop` the server** before the monitor finishes —
  killing the writer mid-stream truncates the log.
