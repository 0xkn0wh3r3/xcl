//! Determinism of the direct translator (same input → byte-identical containers, in
//! separate translator instances and threads) and builder misuse reported as errors.

use super::kernel::{self, KernelOptions};
use crate::gpu::dxbc::SigElement as S;
use crate::gpu::rdna2::encode;
use crate::gpu::recomp::{CsInterface, PsInterface, VsInterface};

fn prog(list: &[(&str, &str, &[(&str, u64)], Option<u32>)]) -> Vec<u8> {
    let mut v = Vec::new();
    for (n, e, f, l) in list {
        v.extend(encode(n, e, f, *l).unwrap_or_else(|x| panic!("{n}: {x}")));
    }
    v.extend(encode("S_ENDPGM", "ENC_SOPP", &[], None).unwrap());
    v
}

fn el(name: &str, index: u32, sv: u32, ct: u32, reg: u32, mask: u8) -> S {
    S { name: name.into(), index, system_value: sv, component_type: ct, register: reg, mask, rw_mask: mask, stream: 0 }
}

/// Programs touching many registers (hash-map keyed state), branches, guarded memory,
/// LDS, cross-lane reads, and VS / PS signatures.
fn translate_all() -> Vec<Vec<u8>> {
    const V: u64 = 256;
    let branchy = prog(&[
        ("S_MOV_B32", "ENC_SOP1", &[("SDST", 0), ("SSRC0", 128)], None),
        ("S_MOV_B32", "ENC_SOP1", &[("SDST", 1), ("SSRC0", 138)], None),
        ("S_ADD_U32", "ENC_SOP2", &[("SDST", 0), ("SSRC0", 0), ("SSRC1", 1)], None),
        ("S_SUB_U32", "ENC_SOP2", &[("SDST", 1), ("SSRC0", 1), ("SSRC1", 129)], None),
        ("S_CMP_LG_U32", "ENC_SOPC", &[("SSRC0", 1), ("SSRC1", 128)], None),
        ("S_CBRANCH_SCC1", "ENC_SOPP", &[("SIMM16", 0xFFFC)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 1), ("SRC0", 0)], None),
        ("V_CMP_GT_U32", "ENC_VOPC", &[("SRC0", 160), ("VSRC1", 0)], None),
        ("S_AND_SAVEEXEC_B64", "ENC_SOP1", &[("SDST", 20), ("SSRC0", 106)], None),
        ("GLOBAL_LOAD_DWORD", "ENC_FLAT_GLBL", &[("VDST", 5), ("ADDR", 0), ("SADDR", 0x7D), ("OFFSET", 8)], None),
        ("V_ADD_F32", "ENC_VOP2", &[("VDST", 10), ("SRC0", V + 5), ("VSRC1", 2)], None),
        ("V_READLANE_B32", "ENC_VOP3", &[("VDST", 30), ("SRC0", V + 10), ("SRC1", 129)], None),
        ("S_MOV_B64", "ENC_SOP1", &[("SDST", 126), ("SSRC0", 20)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 12), ("SRC0", 30)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 13), ("SRC0", 31)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 14), ("SRC0", 32)], None),
    ]);
    let vs_code = prog(&[
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 0), ("SRC0", V + 4)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 1), ("SRC0", V + 5)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 2), ("SRC0", 128)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 3), ("SRC0", 242)], None),
        ("EXP", "ENC_EXP", &[("TGT", 12), ("EN", 0xF), ("VSRC0", 0), ("VSRC1", 1), ("VSRC2", 2), ("VSRC3", 3), ("DONE", 1)], None),
        ("EXP", "ENC_EXP", &[("TGT", 32), ("EN", 0xF), ("VSRC0", 8), ("VSRC1", 9), ("VSRC2", 10), ("VSRC3", 11)], None),
        ("EXP", "ENC_EXP", &[("TGT", 33), ("EN", 0x3), ("VSRC0", 0), ("VSRC1", 1), ("VSRC2", 0), ("VSRC3", 0)], None),
    ]);
    let vsi = VsInterface {
        inputs: vec![el("POSITION", 0, 0, 3, 0, 3), el("COLOR", 0, 0, 3, 1, 0xF)],
        outputs: vec![el("SV_Position", 0, 1, 3, 0, 0xF), el("COLOR", 0, 0, 3, 1, 0xF), el("TEXCOORD", 3, 0, 3, 2, 0x3)],
        attr_base: 4,
        params: None,
    };
    let ps_code = prog(&[
        ("V_INTERP_P2_F32", "ENC_VINTRP", &[("VDST", 4), ("VSRC", 1), ("ATTR", 0), ("ATTRCHAN", 0)], None),
        ("V_INTERP_P2_F32", "ENC_VINTRP", &[("VDST", 5), ("VSRC", 1), ("ATTR", 0), ("ATTRCHAN", 1)], None),
        ("V_INTERP_P2_F32", "ENC_VINTRP", &[("VDST", 6), ("VSRC", 1), ("ATTR", 1), ("ATTRCHAN", 0)], None),
        ("V_MOV_B32", "ENC_VOP1", &[("VDST", 7), ("SRC0", 242)], None),
        ("EXP", "ENC_EXP", &[("TGT", 0), ("EN", 0xF), ("VSRC0", 4), ("VSRC1", 5), ("VSRC2", 6), ("VSRC3", 7), ("DONE", 1), ("VM", 1)], None),
    ]);
    let psi = PsInterface {
        inputs: vec![el("SV_Position", 0, 1, 3, 0, 0xF), el("COLOR", 0, 0, 3, 1, 0xF), el("TEXCOORD", 3, 0, 3, 2, 0x3)],
        outputs: vec![el("SV_Target", 0, 64, 3, 0, 0xF)],
        input_ena: 0x302,
        attrs: None,
    };
    let cs = CsInterface { threads: [64, 1, 1], user_sgprs: 4, tgid: [true, false, false], tg_size: false, tid_components: 1 };
    let o = |wave64, split| KernelOptions { wave64, split, mem_windows: 1, sampler_base: 0 };
    let mut out = Vec::new();
    for (w, s) in [(false, false), (true, true), (true, false)] {
        out.push(kernel::translate(&branchy, o(w, s)).expect("kernel"));
        out.push(kernel::translate_compute(&branchy, o(w, s), &cs).expect("compute").dxil);
    }
    out.push(kernel::translate_vertex(&vs_code, o(true, false), &vsi).expect("vs").dxil);
    out.push(kernel::translate_pixel(&ps_code, o(true, false), &psi).expect("ps").dxil);
    out
}

#[test]
fn translation_is_byte_identical_across_runs_and_threads() {
    let first = translate_all();
    for _ in 0..3 {
        assert!(translate_all() == first, "a second translation differs");
    }
    let threads: Vec<_> = (0..4).map(|_| std::thread::Builder::new().stack_size(64 << 20).spawn(translate_all).unwrap()).collect();
    for t in threads {
        assert!(t.join().unwrap() == first, "a translation on another thread differs");
    }
}
