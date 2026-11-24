# Direct DXIL — dx.op library progress (branch `dxil/ops`)

Code: `crates/xcl/src/gpu/dxil/ops.rs` (helpers), `ops_tests.rs` (tests).
Reference: DXC 1.9 output for small HLSL (`-T cs_6_6 -Fc`), `docs/DXIL.rst`
(NCSA). No VIctory code used.

## Helpers and test status

V = DXC validator, G = GPU run on RTX 4090 (wave32) vs CPU reference.

| Area | Helpers | Status |
|---|---|---|
| Float unary | `funary(FUnary::{FAbs,Saturate,Frc,Round*,Cos,Sin,Exp,Log,Sqrt,Rsqrt,trig,hyperbolic})` | V G (exact ops bit-compared, approximate with tolerance) |
| f64 unary legality | `DOUBLE_UNARY` = FAbs, Saturate | V (probe test) |
| Float class | `fclass(IsNaN/IsInf/IsFinite/IsNormal)` | V G |
| Bits | `bfrev`, `bit_count(Countbits/FirstbitLo/Hi/SHi)` (i32, i64) | V G |
| Binary | `binary(FMax/FMin/IMax/IMin/UMax/UMin)` (f16/f32/f64/i32/i64) | V G |
| Tertiary | `tertiary(FMad/Fma/IMad/UMad/Ibfe/Ubfe)`, `bfi` | V G |
| Wide | `umul_wide`, `imul_wide`, `udiv_rem`, `uaddc`, `usubb` | V G (UAddc/USubb accepted by DXC 1.9 validator) |
| Doubles | `make_double`, `split_double`, Fma (sets `DoubleExtensions`) | V G |
| Half | `f32_to_f16`, `f16_to_f32`; native f16 overloads | V G |
| Groupshared | `groupshared`, `gs_ptr/load/store/atomic/cmpxchg` (i32, i64) | V G |
| Barrier | `barrier(mode)` + `barrier::*` flags | V G |
| Typed buffers | `typed_uav`, `heap_typed_buffer`, `buffer_load/store`, `typed_atomic` | V + GPU (`typed_buffer_ops_run`, `runner::TexDim::Buffer`) |
| Structured | `structured_uav`, `heap_structured_buffer`, `structured_load/store/atomic` | V G |
| Raw atomics | `raw_atomic`, `raw_cmpxchg` (i32, i64), generic `atomic_binop`/`atomic_cmpxchg` | V G |
| Textures | `heap_texture` (1D…CubeArray, SRV/UAV), `heap_sampler`, `texture_load/store`, `sample`, `sample_bias`, `sample_level`, `sample_grad`, `sample_cmp`, `sample_cmp_level_zero`, `gather`, `gather_cmp`, `get_dimensions`, `calculate_lod` | V G (2D, 2D array, 3D, cube SRV; 2D / 2D array / 3D UAV, texture atomics); offsets/clamp/unclamped LOD V only |
| Wave | `wave_is_first_lane`, `wave_lane_count`, `wave_any/all_true`, `wave_all_equal`, `wave_active_op`, `wave_active_bit`, `wave_prefix_op`, `wave_all_bit_count`, `wave_prefix_bit_count`, `quad_read_lane_at`, `quad_op`, `wave_match`, `wave_multi_prefix_op`, `wave_multi_prefix_bit_count` (i32/i64/f16/f32) | V G |
| Derivatives | `deriv(CoarseX/Y, FineX/Y)` | V G in CS (numthreads 8×4: 2×2 quads); PS untested (no graphics entry yet) |

## Shared-file changes (`shader.rs`, additive)

- `Feature::{DoubleExtensions (bit 6 / 0x20), TiledResources (12 / 0x100), TypedUavLoadAdditionalFormats (13 / 0x800)}`.
- Fixed `AtomicInt64Heap` metadata bit: 32 (was 33), per DXC output.
- `NativeLowPrecision` also sets `LowPrecisionPresent` (bit 5), as the validator requires.
- `Binding` gained `comp_type` (typed element type, metadata tag 0) and `atomic64` (tag 3 + PSV `UsedByAtomic64` flag); typed-buffer bindings get kind 10 / `UavTyped` in PSV0.
- `RawUav` carries its binding index; a few items became `pub(super)` (`bindings`, `handle_ty`, `handle_from_binding`, `res_ret`, `type_suffix`).

## Findings

- Validator rejects reading undefined `GetDimensions` components (e.g. `z` of Texture2D): the helper returns the struct; callers extract.
- 64-bit atomics: raw/structured bindings set only `Int64Ops` + metadata tag 3 + PSV flag; typed resources add `AtomicInt64Typed`; heap resources add `AtomicInt64Heap`.
- RTX 4090 `f32tof16` rounds towards zero.
- `isSpecialFloat.f16` needs SM 6.9.
- `Sample` with a clamp operand requires `TiledResources`.

## Open

- Static SRVs (`srv_raw/structured/typed/texture`), CBVs (`cbv`, `heap_cbv`,
  `cbuffer_load_legacy` op 59) and samplers (`sampler`, incl. comparison):
  PSV0/SFI0 byte-checked against `tests/data/dxil/dxc_{cs,ps}_resources.dxil`.
- Pixel-shader derivative / implicit-LOD validation waits for graphics entry points.
- `SampleCmpLevel` (SM 6.7), `TextureGatherRaw`, `CheckAccessFullyMapped` status are not wrapped.
