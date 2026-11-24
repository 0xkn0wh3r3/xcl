# Direct DXIL — graphics stages progress (2026-11-24)

Branch `dxil/graphics` (from `shaders/direct-dxil`). Roadmap phases 5/6 of
`2026-11-24-direct-dxil.md`: vertex, pixel and mesh entry points.

## Code

| File | Content |
|---|---|
| `dxil/graphics.rs` (new) | `SigElement`/`SemKind`/`CompType`/`Interp`, `GraphicsIo` (declare inputs/outputs/primitive outputs, I/O intrinsics, packing), `StageInfo`, `Shader::finish_graphics`, PSV0 dependency-table analysis |
| `dxil/graphics_tests.rs` (new) | byte-for-byte part checks against DXC, validator, GPU draws |
| `dxil/container.rs` (additive) | `SigPartElement` + `signature_part`, `PsvSigElement` + `PsvFull` + `psv_part_full`, `container_with_signatures` (adds `PSG1`) |
| `dxil/module.rs` (additive) | introspection (`InstInfo`, `block_count`, `block_insts`, `inst_info`, `const_u64`), `gep_fields` (struct GEPs), `alloc_size`, `global_bytes` |
| `dxil/shader.rs` | visibility only: `sm`/`entry`/`features`/`bindings`, `Binding` fields and `Feature::{metadata,container}_bit` are `pub(super)` |
| `dxil/mod.rs` | `pub mod graphics;` |

`Shader::finish()` (compute) is unchanged.

## API sketch

```rust
let mut s = Shader::new(Stage::Vertex, (6, 6));
let mut io = GraphicsIo::new(Stage::Vertex);
let pos = io.input(SigElement::user("POSITION", 0, CompType::F32, 3));
let vid = io.input(SigElement::new("SV_VertexID", 0, SemKind::VertexId, CompType::U32, 1));
let out = io.output(SigElement::new("SV_Position", 0, SemKind::Position, CompType::F32, 4));
let x = io.load_input(&mut s, pos, /*row*/ 0, /*col*/ 0);
io.store_output(&mut s, out, 0, 0, x);
let dxil = s.finish_graphics(io, StageInfo::Vertex);
```

Elements: `.interp(Interp::…)` (default = DXC's: VS in / PS out undefined,
float linear, `SV_Position` noperspective, ints and primitive outputs
constant), `.rows(n)` (consecutive semantic indices), `.at(row, col)` (fixed
placement, otherwise DXC packing). Ops: `load_input` (PS `SV_SampleIndex`
→ `sampleIndex`), `store_output`, `discard`, `coverage`, `inner_coverage`,
`is_helper_lane`, `set_mesh_output_counts`, `emit_indices` (2 or 3),
`store_vertex_output`, `store_primitive_output`, `mesh_payload`.

## Rules learned from DXC 1.9 output (NCSA; references in `tests/data/dxil/`)

- **Packing** (default prefix-stable): elements in declaration order, first
  fit from row 0, column 0. A row holds one interpolation mode; columns fill
  left to right; nothing but another system value may follow a system value
  in a row (arb then `SV_RenderTargetArrayIndex` is fine, not the reverse);
  clip/cull distances share rows only with each other. VS inputs are not
  packed (one row each). `SV_Target` row = semantic index. Unallocated (row
  and column −1, register `0xFFFFFFFF`): PS `SV_Depth*`, `SV_Coverage` and
  `SV_StencilRef` outputs, PS `SV_SampleIndex` input, MS `SV_CullPrimitive`.
  PS `SV_Coverage`/`SV_InnerCoverage` inputs are not signature elements.
- **ISG1/OSG1/PSG1**: 32-byte elements, one per row, sorted by (register,
  start column) with unallocated last; names deduplicated, stored in
  declaration order. Mask byte 2 = inputs: components read; outputs:
  `~written & 0xF` (start-column shifted); unallocated inputs 0.
- **PSV0**: elements in declaration order; system values have name offset 0;
  string table not deduplicated (names, then entry); semantic index table
  reuses an existing run. Vectors = allocated rows (a lone `SV_Target2`
  counts 3). Dependency tables: per input component (vectors × 4) a bitmask
  over output components, only when both counts are non-zero; unallocated
  outputs never appear. Stage union: VS `OutputPositionPresent`; PS
  `DepthOutput` (Depth/LE/GE), `SampleFrequency` (sample interpolation or
  `SV_SampleIndex`); MS groupshared bytes, payload bytes, max vertices /
  primitives, and in the `RuntimeInfo1` union primitive vectors + topology.
- **Dependencies** (validator recomputes and compares them with PSV0 —
  confirmed by corrupting them): data flow, memory (loads see every store
  into the same root global), control dependence via post-dominators, and
  for phis the branch condition of each incoming block.
- `dx.viewIdState` = `[inputs, outputs, (MS: primitive outputs), table]`
  with component counts `(last row) * 4 + start col + cols`.
- Shader flags: VS with a bound (not heap-indexed) UAV → bit 16 + SFI0
  `0x4` (not for MS); VS (not MS) writing RT/viewport index → bit 9 and SFI0
  `0x2000`; PS `SV_StencilRef` → bit 11, SFI0 `0x200`; `[earlydepthstencil]`
  → bit 3. `SV_CullPrimitive` is `i1` in metadata but stored as `i32`.
  Line `emitIndices` pads the third index with 0.
- 16-bit elements: container component types 4/5/6 (u16/i16/f16); min
  precision 1/4/5 plus `LowPrecisionPresent` (bit 5) and SFI0 `0x10`, or 0
  with `Feature::NativeLowPrecision`, which also switches the data layout
  (`DATALAYOUT_NATIVE16`, `i8:8-i16:16-f16:16`).
- `SigElement::from_d3d` maps DXBC-style rows (`D3D_NAME`, component type,
  mask) for the translator.

## Status

`cargo test -p xcl --lib gpu::dxil`: 47 tests pass (31 graphics + the ops/compute suites after merging `shaders/direct-dxil`).

- Byte-identical ISG1/OSG1/PSG1/PSV0 (and SFI0 where flags matter) with DXC
  for: `vs_basic`, `ps_basic`, `ps_modes` (all interpolation modes,
  IsFrontFace/PrimitiveID, discard, coverage, 2 targets, depth),
  `ps_packing` (SV after arb, mixed types, unpacked SampleIndex, unused
  input), `vs_packing` (RT index, 2-row matrix, InstanceID),
  `vs_clip`/`vs_clip_rows`, `vs_control` (branches + loop), `ps_outputs`
  (Target2, coverage/depth-GE/stencil outputs, centroid position),
  `ps_early`, `ps_min16`/`ps_native16`, `vs_uav` (bound UAV in a VS → UAVs-at-every-stage flag), `ms_basic`, `ms_payload` (payload, groupshared, lines,
  CullPrimitive, RT index in a primitive row). All validate.
- GPU (RTX 4090): emitted VS+PS triangle via `runner::draw` (provoking-vertex
  `nointerpolation`, root UAV read in the PS); emitted MS+PS via
  `runner::mesh_draw`; MS primitive attribute read by a PS with discard;
  PS implicit-LOD `sample` + `ddx_fine` from the `ops` library through
  `runner::draw_on_multi` (heap texture + sampler) — the PS-only paths the
  ops branch could not exercise.

## Open

- Dynamic row indexing of inputs/outputs (DynIdx masks) not supported
  (rows are constants in the API).
- Not covered: GS/HS/DS/AS, `SV_ViewID`/UsesViewID tables,
  `SV_Barycentrics`, `SV_ShadingRate`, `SV_InnerCoverage` flag bit.
- Groupshared bytes use `Module::global_bytes_layout(3, native16)`.
- Resources: both finish paths use `Shader::resource_metadata` (shader.rs).
- `mesh_payload` cannot be exercised on the GPU bench (no amplification
  stage); validated only.
