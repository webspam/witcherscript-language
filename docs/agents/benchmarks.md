# Benchmarks

| File | Tool | Where | Purpose |
|---|---|---|---|
| `benches/lib_*.rs` | criterion | Local | Wall-clock timing of parse / symbols / index / resolve / completion. Use during refactor iteration with `just bench-baseline` and `just bench-compare`. |
| `benches/iai_lib.rs` | iai-callgrind | CI (Linux) + local on WSL | Instruction counts under cachegrind. Deterministic; immune to CI runner noise. Regression-gating layer. |
| `benches/lsp_smoke.rs` | criterion | Local only | Spawns the release `witcherscript-lsp` binary and measures cold start + warm request latency over stdio. Not gated. |
| `benches/common/synth.rs` | helper | n/a | Deterministic `synth_file` / `synth_workspace` generators. Each bench includes it via `#[path = "common/synth.rs"]`. |

The wire-level smoke bench shares `src/bin/witcherscript-lsp/tests/jsonrpc_client.rs` with the E2E harness: same framing code, two transports.

## iai-callgrind CI comparison

iai-callgrind writes per-bench instruction counts under `target/iai/`. CI uses iai-callgrind's *named baseline* mechanism: master records a baseline called `master`, PRs compare against it. The comparison is **advisory**: it never blocks merging, just prints the diff to the job log.

- Cache key is `iai-<os>-master-<commit-sha>`. Restore key is the prefix `iai-<os>-master-`, so any run resolves to the most recent master baseline.
- Only `push` events on `master` save the cache: master runs `cargo bench -- --save-baseline=master` and persists `target/iai/`. PR runs restore but never save, so a PR cannot poison the baseline.
- PR runs compare with `cargo bench -- --baseline=master`.
- If a PR finds no cache (e.g. GitHub expired it after a week), the job checks out `master`, runs `--save-baseline=master` to seed `target/iai/`, then checks back out to the PR HEAD before the comparison run.
- `IAI_CALLGRIND_REGRESSION=ir=5.0` flags any bench whose instruction reads climb >5% vs `master`. The job tolerates that flag (exit code 3) and stays green.

The benchmark binary needs symbols for cachegrind to attach its collection toggle, so `[profile.bench]` overrides release with `strip = false` / `debug = true` and `lto = false` (under LTO an unrelated edit can shift inlining and produce phantom drift).

Fixture work inside each `#[library_benchmark]` runs in a `#[bench(setup = ...)]` callback so cachegrind only measures the target call, not the surrounding parse / index build.

Recipes:

```
just bench                   # criterion library benches (wall-clock)
just bench-baseline pre      # save a labelled criterion baseline
just bench-compare pre       # compare current run against saved baseline
just bench-iai               # iai-callgrind (requires valgrind, Linux/WSL)
just bench-lsp               # criterion LSP smoke benches against release binary
```

CI runs only `iai_lib` (on `ubuntu-latest` with valgrind installed). Criterion benches are local; CI wall-clock measurement is too noisy to gate on.

When adding a new hot path that needs perf coverage: add a criterion bench under `benches/lib_<area>.rs` for local iteration, an iai-callgrind bench in `benches/iai_lib.rs` for CI gating, and a scenario in `benches/lsp_smoke.rs` if the path is only reached through the LSP wire protocol.
