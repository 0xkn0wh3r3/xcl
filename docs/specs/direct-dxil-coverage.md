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
  skips big programs, which can take minutes and many GB each right now.
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

## 2026-11-24 sample: `D:\game\shaders.rpf`, 200 records, seed 1

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
