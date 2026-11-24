//! Direct DXIL emission: console GPU code → DXIL containers without HLSL or
//! DXC. Design and roadmap: `docs/specs/2026-11-24-direct-dxil.md`.
//!
//! Layers, bottom up:
//! * [`bitstream`] — LLVM bitstream writer (unabbreviated records only);
//! * [`module`] — an LLVM 3.7 module (types, constants, one entry function,
//!   metadata) and its bitcode;
//! * [`shader`] — DXIL on top: `dx.op` intrinsics, resource handles,
//!   entry-point metadata, shader flags;
//! * [`container`] — the DXBC container (`SFI0`, signatures, `PSV0`,
//!   `DXIL`) and its digest, so no SDK DLL is needed at run time;
//! * [`validate`] — `dxil.dll` validator and DXC disassembler as test oracles.

pub mod bitstream;
pub mod container;
pub mod graphics;
pub mod module;
pub mod ops;
pub mod shader;
pub mod validate;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod bitreader_tests;
