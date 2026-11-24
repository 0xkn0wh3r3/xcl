# Direct DXIL — retail coverage

Tooling: `xcl shaders <pack|folder|file> --direct` (`crates/xcl/src/shaders.rs`
`direct_sweep`, aggregation in `crates/xcl/src/gpu/dxil/coverage.rs`).

```
xcl shaders <pack.rpf> --direct [--sample N] [--seed S] [--validate] [--compare]
            [--json] [--top K] [--threads T] [--timeout SEC] [--max-bytes B]
            [--windows W] [--split 0|1] [--trace]
```

* `--sample N` (default 200, `0` = everything): seeded sample of records; stops at
  N programs. `--seed` (default 1) picks the sample. With several threads the last
  few programs taken can differ run to run; use `--threads 1` for an exact repeat.
* Every program found by `recomp::interface` goes through `coverage::translate_mode`
  (panics caught). Outcomes per stage (CS / VS / PS / MS / GS / kernel):
  translated, unsupported, validator (`--validate` runs DXC's validator; debug
  builds always validate), panicked, timeout.
* `--timeout` (default 10 s) per program; `--max-bytes` (default 16384, 0 = no cap)
  skips big programs (since the fixes below the biggest take a few seconds; `0` is fine
  for a 200-program sample).
* Unsupported histogram: the error text with operands / numbers stripped, ranked by
  programs, with the stages and one example program.
* Time per program p50 / p95 / max (timeouts count as the timeout).
* `--compare`: the HLSL path (`recomp::translate` + DXC) for each program the direct
  path translated; total DXIL size of both.
* `--json`: the same as one JSON object.

Live (`XCL_DIRECT_DXIL=1|all`): `pso.rs` logs every fallback as
`[SHADER] direct DXIL fallback stage=CS reason="…" detail="…"` and counts
direct / fallback per stage. `XCL_DIRECT_DXIL_STATS=1` prints the summary
(ranked reasons) to stderr and the log at exit (`report::write_final`).

## After the sweep fixes (2026-11-24): same pack, `--sample 200 --seed 1 --max-bytes 0 --timeout 10`

`xcl shaders D:\game\shaders.rpf --direct --sample 200 --seed 1 --max-bytes 0 --timeout 10 --compare`
(release, 132 s). All 200 programs tried (no size cap). With `--validate` instead of
`--compare`: the same 197 pass DXC's validator (11 s).

| stage | total | translated | unsupported | validator | panicked | timeout |
|---|---|---|---|---|---|---|
| CS | 21 | 21 (100%) | 0 | 0 | 0 | 0 |
| GS | 3 | 0 | 3 | 0 | 0 | 0 |
| MS | 67 | 67 (100%) | 0 | 0 | 0 | 0 |
| PS | 86 | 86 (100%) | 0 | 0 | 0 | 0 |
| VS | 23 | 23 (100%) | 0 | 0 | 0 | 0 |
| all | 200 | 197 (98.5%) | 3 | 0 | 0 | 0 |

Translation time: p50 64 ms, p95 1.6 s, max 3.7 s.
DXIL size, 197 programs: direct 30.7 MB, HLSL+DXC 24.1 MB (**1.27×**).
The only failures left: geometry mode (ES+GS), 3 programs.

Same command before the fixes: 113 / 200 translated (56.5%; 30 forward-reference
panics, 54 timeouts over 10 s), p50 605 ms, p95 10.0 s (the timeout), direct/HLSL
size 14.5× (99.4 MB vs 6.8 MB for the 113 programs).

What changed (`dxil: fix VS/PS block order panics, …`):

* Blocks are written in reverse post-order (unreachable ones dropped), so the
  VS/PS epilogue — created before the program's blocks — may read their values.
* SSA: a sealed join reads its operands first and makes a phi only when they
  differ or a loop comes back around. Before, every join a read walked through got
  a phi per register: the bulk of both the time and the size.
* `Module::optimize` before finishing: trivial phis, CSE (pure ops along the
  dominator tree, wave ops within a block), DCE of pure values and of dx.op calls
  that set no shader flag. `XCL_DXIL_NO_OPT=phi,cse,dce` (or `all`) disables passes.
* Unreferenced globals / function declarations are not written
  (`Internal declaration 'xcl_xs' is unused`); `SV_IsFrontFace` is declared uint.
* The view-id control dependence uses an immediate post-dominator tree instead of
  a post-dominator set per block (quadratic time and memory: the 28 GB runs).

Remaining time is the per-register SSA walk over the many blocks guarded memory
accesses create (merging adjacent guards would cut it).

## 2026-11-24 sample (before the fixes): `D:\game\shaders.rpf`, 200 records, seed 1

`xcl shaders D:\game\shaders.rpf --direct --sample 200 --timeout 5 --validate --compare`
(release, RTX 4090 machine, 71 s with `--compare`; 21 s without).
167 programs, 33 programs over 16 KiB skipped.

| stage | total | translated | unsupported | validator | panicked | timeout |
|---|---|---|---|---|---|---|
| CS | 14 | 11 (78.6%) | 0 | 1 | 0 | 2 |
| GS | 3 | 0 | 3 | 0 | 0 | 0 |
| MS | 66 | 66 (100%) | 0 | 0 | 0 | 0 |
| PS | 61 | 31 (50.8%) | 0 | 2 | 13 | 15 |
| VS | 23 | 4 (17.4%) | 0 | 0 | 19 | 0 |
| all | 167 | 112 (67.1%) | 3 | 3 | 32 | 17 |

Translation time: p50 366 ms, p95 5.0 s (the timeout), max 5.0 s.
DXIL size, 112 programs both paths produced: direct 92.3 MB, HLSL+DXC 6.8 MB
(**13.6×** larger — the direct output is unoptimized).

Ranked failures:

1. 32 — panic `module.rs:886` "forward reference to value N at instruction N
   (blocks must be in dominance order)" — PS 13, VS 19 (e.g.
   `PS_DeferredTextured_c5fd36bb_Wrapped`). The biggest gap: VS/PS block ordering.
2. 17 — timeout over 5 s — PS 15, CS 2 (e.g. `PS_AlphaPrepassSpatter_Fire_26a320a2_Wrapped`);
   plus the 33 programs over 16 KiB not tried. Translation time grows badly with size.
3. 3 — geometry mode (ES+GS) not supported.
4. 2 — validator: `SV_IsFrontFace must be uint` (PS, e.g. `PS_Imposter_827db145_Wrapped`).
5. 1 — validator: `Internal declaration 'xcl_xs' is unused` (CS,
   `CS_FlattenTest_d36b6de5_Wrapped`).
