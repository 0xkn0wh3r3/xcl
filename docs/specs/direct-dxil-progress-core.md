# Direct DXIL — translator core progress (phases 1–3, parts of 4–6)

Branch `dxil/core`. Code: `crates/xcl/src/gpu/dxil/` — `cfg.rs`, `ssa.rs`,
`lanes.rs`, `lower.rs` (+ `lower_valu.rs`, `lower_wide.rs`, `lower_mem.rs`,
`lower_image.rs`, `lower_gfx.rs`), `prelude.rs`, `kernel.rs`, `ops_core.rs`,
`core_tests.rs`.

Run: `XCL_DIRECT_DXIL=1 cargo test -p xcl --lib gpu::recomp::tests`
(RTX 4090: wave32 hardware only, so wave64 programs run in the split form;
the two tests that need 64-lane hardware skip).

## Milestones

| Date | Commit | pass / fail / skip (of 100 run) | direct-path kernels passing |
|---|---|---|---|
| 2026-11-24 | first wiring (SALU, VALU f32/int, branches) | 56 / 44 / 2 | — |
| 2026-11-24 | `66b9e14` + f16 / f64 / packed / movrel | 77 / 23 / 2 | 28 / 51 |
| 2026-11-24 | `0f2f999` memory: SMEM, MUBUF/MTBUF (formats, d16), FLAT/global, LDS, atomics, exports | 92 / 8 / 2 | 43 / 51 |
| 2026-11-24 | `911721f` scratch (`alloca`) | 93 / 7 / 2 | 44 / 51 |
| 2026-11-24 | `acb04ea` images (heap views), BVH | 99 / 1 / 2 | 50 / 51 |
| 2026-11-24 | `8a694c6`, `004d37b` static samplers, f16 round mode, memory windows; compile-only / translate+run tests routed too | 100 / 0 / 2 | all routed kernels |
| 2026-11-24 | `49810fe` compute mode (CS ABI) incl. the split form's spin exchange; 5 compute tests routed | 100 / 0 / 2 | all routed |
| 2026-11-24 | `0b6a4bc` live: console compute shaders through the direct path (`pso.rs`, HLSL fallback) | 100 / 0 / 2 | — |
| 2026-11-24 | `85b2f80` mesh mode (NGG first pass) + live mesh pass | 100 / 0 / 2 | — |
| 2026-11-24 | `0f53a7e` VS / PS entries; `XCL_DIRECT_DXIL=all` routes console VS/PS | 100 / 0 / 2 | — |

Default path (`cargo test -p xcl --lib gpu::recomp::tests` without the
variable): 100 passed, 14 ignored — unchanged. `cargo test -p xcl --lib
gpu::dxil`: 56 passed, 1 ignored (a DXC-lowering dump helper).

## What goes through the direct path in the tests

Every recomp test that runs or compiles a kernel-, compute-, mesh-, vertex- or
pixel-mode program:
`runner::run_code*`, `run_program`, `kernel_wave`, and the `direct_or` /
`direct_check` hooks in `recomp/tests.rs` for the tests that translate + run
(`runner::run`, `run_in_arena`, `dispatch`) or only compile. Under
`XCL_DIRECT_DXIL=1` a translation error is a test failure (no fallback).

## Remaining failures (direct path)

None: 100 passed, 0 failed, 2 skipped (`gpu_geometry_wave32_prefix_count_over_a_mask`,
`gpu_wave32_program_in_wave64_hardware` need 64-lane hardware), 14 ignored
(surveys / benches).

Not covered (the translator errors, or the tests keep the HLSL path):
geometry mode (ES+GS), `hw_wave = 64` (a wave32 program in 64-lane
hardware), native wave64 hardware (code paths exist, untested here), MSAA
image sampling, non-default f32 round modes (unsupported by the HLSL path
too). VS / PS are covered by one GPU draw (`gpu_graphics_vs_ps_draw`), two
compile-only PS tests and `core_tests::direct_and_hlsl_graphics_stages_link`
(direct and HLSL stages link both ways) — thinner than compute. The corpus
sweep (`retail_translation_survey`) needs `XCL_SAMPLE_RPF`, not set on this
machine, so retail coverage is unmeasured.

## Design notes

* **Model.** Faithful to the HLSL kernel: every console lane is a thread; SGPRs,
  SCC, VCC, M0, EXEC are wave-uniform SSA values; VGPR writes are
  `select(exec_bit, new, old)`; lane masks are ballots. Scalar branches are
  uniform DXIL branches. EXEC-dependent memory accesses are real per-lane
  branches (`Lw::guarded`), so disabled lanes never touch memory. Where EXEC
  is a known constant (full compute waves, `s_mov_b64 exec, -1`) constant
  folding removes the lane selects and guard branches.
* **SSA.** Braun et al. over *module* blocks (not just machine blocks):
  internal diamonds (guarded memory, CAS loops) get their phis for free.
  Blocks are created lazily at first reference, which keeps the module's
  positional value numbering dominance-ordered. Phis are inserted at block
  front with the new `Module::phi_at`. The entry block stays open to the end:
  initial register loads from `io` are emitted there on first read.
  Translation runs on a 256 MB-stack thread (the read recursion is one level
  per guarded access between a definition and its use).
* **Split wave64.** Lanes are assigned per arriving hardware wave through a
  groupshared counter (as `xcl_x_thread`); ballots, `v_readlane` /
  `v_readfirstlane`, coherent scalar loads and `ds_bpermute` exchange through
  groupshared words: between two group barriers when the group is one console
  wave, through arrival counters (bounded spin, double-buffered slots, dead
  flag — the HLSL `XCL_X_SPIN` protocol) when it has several. Masks only read
  lane by lane skip the exchange (`recomp::x64_masks` liveness). With LDS the
  exchange words live in the top of `xcl_lds` and LDS indices clamp below them
  (same layout rule as the HLSL kernel). DPP / swizzle / permlane read within
  the half.
* **Store set (kernel mode).** Registers named by any emitted instruction (all
  of them in a branchy program, up to the first `s_endpgm` in straight-line
  code) are loaded and stored, so untouched `res` slots stay 0 as with HLSL.
* **Compute mode.** User SGPRs from `io`, group ids / TG_SIZE after them, SCC 0,
  EXEC = the wave's threads, v0..v2 = thread id (2-D / 3-D groups and the split
  form renumber console threads as the HLSL entry does); memory windows are
  `globallycoherent`.
* **Exactness.** No fast-math flags. Helpers port `PRELUDE` operation for
  operation; where HLSL intrinsics were involved DXC's lowering was checked
  (`core_tests::dump_dxc_lowering`, e.g. `frexp` drops the mantissa sign,
  `ldexp(x, e)` = `x * exp2(e)`, `dot` = `Dot3`). f64 conversions / division /
  fma require the double-extensions flag.
* **Mesh mode.** One record per group in `res` (header from GS_ALLOC_REQ,
  vertices from `exp pos/param`, primitives from `exp prim`), slots as the
  HLSL mesh entry computes them; the pass-through mesh shader stays HLSL.
* **VS / PS.** Signatures declared exactly as the HLSL entry structs (inputs
  `a<reg>` / `a<k>` of 4 components, SV ids, `p<k>` outputs, SV_Target<i>),
  through `graphics.rs`, so a direct stage links with an HLSL one. Outputs are
  temporaries stored once at the end (unwritten = 0). PS: `v_interp_*` read
  the interpolated inputs (modes from the I/J pair, `emit::ps_attr_modes`),
  lanes without a DONE export are discarded, lane reads within a quad use quad
  reads (helper lanes), samples without LOD use implicit derivatives.
* **Loop budget.** Loop headers decrement a budget (`1 << 16`, as
  `LOOP_GUARD`); at 0 the program ends (the HLSL breaks out of the loop
  instead — only differs for runaway loops).

## Shared-file changes

* `module.rs`: `Module::phi_at(block, ty)`, `Module::phi_incoming`,
  `Module::alloca(ty, align)` (+ `Op::Alloca` record), doc fix: float divide is
  `BinOp::SDiv` (the bitcode reader rejects `UDiv` on floats).
* `shader.rs`: `Shader::raw_uav_coherent` (`Binding::coherent`, metadata
  globallycoherent flag, resource property bit 14).
* `mod.rs`: new modules.
* `recomp/mod.rs`: `constant_samplers` is `pub(crate)`.
* `recomp/runner.rs`: `direct_dxil()`, `direct_kernel()`, `direct_kernel_full()`,
  the switch in `run_code` / `run_code_with`; `recomp/pso.rs`: `direct_compute`,
  `direct_graphics` and the direct mesh pass (each with HLSL fallback);
  `recomp/tests.rs`: the switch in `kernel_wave`, `direct_or` / `direct_check`.

## Switches

* `XCL_DIRECT_DXIL=1`: kernel tests, console compute shaders and the mesh pass
  through the direct translator (live: HLSL fallback, logged).
* `XCL_DIRECT_DXIL=all`: console vertex and pixel shaders too.

## Main-based port

Branch `shaders/direct-dxil-main` (on `origin/main`): the translator from
`dxil/core` (olympus-based) ported onto the main-based emitter.

* `gpu/dxil/`: the translator's changes since its last merge of
  `shaders/direct-dxil` (2919399) applied with 3-way merging; the review fixes
  of the emitter (atomic ordering, shader-flag scan, float-constant masking,
  validate.rs FFI) are kept. No olympus-only recomp/decoder API was needed;
  nothing is gated.
* Hooks re-done against main: `runner.rs` (`direct_dxil`, `direct_kernel`,
  `direct_kernel_full`, the switch in `run_code` / `run_code_with`), `pso.rs`
  (`direct_compute`, `direct_graphics`, direct mesh pass with main's
  `mem_windows()`; FSPX driver containers stay on the HLSL path; with the
  switch on, the on-disk translation cache is bypassed so direct DXIL never
  lands under the HLSL path's keys), `tests.rs` (`direct_or` / `direct_check`;
  the hooks for tests main does not have are dropped), `recomp/mod.rs`
  (`constant_samplers` `pub(crate)`).
* Translator fixes for tests only main has: f64 / `v_cvt_f32_f64` /
  `v_cvt_f16_f32` output modifiers and clamp, unsigned clamp of
  `v_add/sub/subrev_nc_u32` / `v_add3_u32`, packed 16-bit clamp
  (`v_pk_add/sub/mad_{u,i}16`), dot-product clamp; `src_vccz` / `src_execz`
  ignore the high half in wave32 and are readable as 64-bit sources;
  `s_*_saveexec_b32` with a 32-bit source (m0); SDWA f16 inline constants;
  cube T#s as cube arrays (`face + 8 · cube`, resinfo layers = 6 · cubes), as
  main's HLSL path.

Results (RTX 4090, wave32):

| Command | pass / fail / ignored |
|---|---|
| `cargo test -p xcl --lib gpu::dxil` | 65 / 0 / 2 |
| `cargo test -p xcl --lib gpu::recomp` | 189 / 0 / 18 |
| `XCL_DIRECT_DXIL=1 cargo test -p xcl --lib gpu::recomp::tests` | 118 / 0 / 17 |
| `XCL_DIRECT_DXIL=all cargo test -p xcl --lib gpu::recomp::tests` | 118 / 0 / 17 |
| `cargo test -p xcl --lib` | 772 / 0 / 23 |
