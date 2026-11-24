# Session goal — direct DXIL backend (set 2026-11-24, ~8 h budget)

## Goal
Real RDNA2 compute kernels translate straight to DXIL and run correctly on the GPU,
with no HLSL and no DXC, proven by XCL's own instruction test suite.

## Done when (all measurable)
1. `XCL_DIRECT_DXIL=1 cargo test -p xcl --lib gpu::recomp::tests`: every test that
   runs on this GPU (RTX 4090, wave32) passes, or has a documented reason it can't yet.
   Target: ≥ 80 % of the non-skipped tests pass on the direct path.
2. Default path unchanged: `cargo test -p xcl --lib` all green (same as `olympus`).
3. Every shader the new path emits passes DXC 1.9's validator.
4. Graphics: a directly emitted VS+PS triangle and a mesh+PS draw give the expected pixels.
5. Op library: math, memory, atomics, texture/sampler and wave helpers, each validated
   and GPU-tested where the bench allows.
6. All branches merged into `shaders/direct-dxil`; roadmap/progress docs updated with
   pass counts and the remaining gaps. Local commits only, no push.

## Plan
- Three subagents in parallel worktrees: `dxil/core` (translator, critical path),
  `dxil/ops` (op library), `dxil/graphics` (signatures, VS/PS/MS).
- Me: review and merge each branch as it lands, fix integration breaks, run the combined
  tests, then hand out follow-up work (e.g. wire core's lowering to the ops and graphics APIs,
  remaining failing tests) until the criteria hold or the time runs out.

## Out of scope this session
Live-title routing default-on, corpus-wide sweeps (too slow per project rules), wave64-only tests.
