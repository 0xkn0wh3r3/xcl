//! On-the-fly SSA construction (Braun, Buchwald, Hack, Leißa, Mallon, Zwinkau:
//! "Simple and Efficient Construction of Static Single Assignment Form", CC 2013)
//! over the module's basic blocks. Variables are register dwords (SGPRs, SCC,
//! VCC, M0, EXEC, VGPRs) numbered by the translator.
//!
//! Blocks are registered as they are created; predecessors are added as the
//! branches are emitted. A block whose predecessors are not all known yet is
//! *unsealed*: reads there create incomplete phis, completed on [`Ssa::seal`].
//! A read that reaches a block without predecessors asks the caller for the
//! variable's initial value (the kernel loads it from `io` in the entry block).

use std::collections::HashMap;

use super::module::{Block, Ty, Value};
use super::shader::Shader;

#[derive(Default)]
struct SBlock {
    defs: HashMap<u32, Value>,
    preds: Vec<Block>,
    sealed: bool,
    /// predecessors to wait for before sealing automatically (None: sealed explicitly)
    expect: Option<usize>,
    incomplete: Vec<(u32, Value)>,
}

pub struct Ssa {
    blocks: Vec<SBlock>,
    ty: Ty,
}

impl Ssa {
    /// All variables have type `ty`.
    pub fn new(ty: Ty) -> Ssa {
        Ssa { blocks: Vec::new(), ty }
    }

    fn at(&mut self, b: Block) -> &mut SBlock {
        let i = b.0 as usize;
        if self.blocks.len() <= i {
            self.blocks.resize_with(i + 1, SBlock::default);
        }
        &mut self.blocks[i]
    }

    /// `b` seals itself once `n` predecessors have been added.
    pub fn expect_preds(&mut self, s: &mut Shader, b: Block, n: usize, init: &mut dyn FnMut(&mut Shader, u32) -> Value) {
        self.at(b).expect = Some(n);
        if self.at(b).preds.len() >= n && !self.at(b).sealed {
            self.seal(s, b, init);
        }
    }

    pub fn add_pred(&mut self, s: &mut Shader, b: Block, p: Block, init: &mut dyn FnMut(&mut Shader, u32) -> Value) {
        let sb = self.at(b);
        assert!(!sb.sealed, "edge into the sealed block {b:?}");
        sb.preds.push(p);
        if let Some(n) = sb.expect {
            if sb.preds.len() >= n {
                self.seal(s, b, init);
            }
        }
    }

    pub fn is_sealed(&mut self, b: Block) -> bool {
        self.at(b).sealed
    }

    pub fn preds(&mut self, b: Block) -> Vec<Block> {
        self.at(b).preds.clone()
    }

    pub fn write(&mut self, b: Block, var: u32, v: Value) {
        self.at(b).defs.insert(var, v);
    }

    /// The definition of `var` reaching the current end of block `b`.
    pub fn read(&mut self, s: &mut Shader, b: Block, var: u32, init: &mut dyn FnMut(&mut Shader, u32) -> Value) -> Value {
        if let Some(&v) = self.at(b).defs.get(&var) {
            return v;
        }
        // chains of sealed single-predecessor blocks (guarded memory accesses make long ones)
        // are walked iteratively, not recursively
        let mut chain = Vec::new();
        let mut b = b;
        loop {
            let sb = self.at(b);
            if !sb.sealed || sb.preds.len() != 1 {
                break;
            }
            chain.push(b);
            b = sb.preds[0];
            if let Some(&v) = self.at(b).defs.get(&var) {
                for c in chain {
                    self.write(c, var, v);
                }
                return v;
            }
        }
        if !chain.is_empty() {
            let v = self.read(s, b, var, init);
            for c in chain {
                self.write(c, var, v);
            }
            return v;
        }
        let v = if !self.at(b).sealed {
            let ty = self.ty;
            let phi = s.m.phi_at(b, ty);
            self.at(b).incomplete.push((var, phi));
            phi
        } else {
            let preds = self.at(b).preds.clone();
            match preds.len() {
                0 => init(s, var),
                1 => self.read(s, preds[0], var, init),
                _ => {
                    let ty = self.ty;
                    let phi = s.m.phi_at(b, ty);
                    self.write(b, var, phi);
                    self.add_operands(s, phi, &preds, var, init);
                    phi
                }
            }
        };
        self.write(b, var, v);
        v
    }

    fn add_operands(&mut self, s: &mut Shader, phi: Value, preds: &[Block], var: u32, init: &mut dyn FnMut(&mut Shader, u32) -> Value) {
        for &p in preds {
            let v = self.read(s, p, var, init);
            s.m.add_incoming(phi, v, p);
        }
    }

    pub fn seal(&mut self, s: &mut Shader, b: Block, init: &mut dyn FnMut(&mut Shader, u32) -> Value) {
        if self.at(b).sealed {
            return;
        }
        let preds = self.at(b).preds.clone();
        let inc = std::mem::take(&mut self.at(b).incomplete);
        for (var, phi) in inc {
            self.add_operands(s, phi, &preds, var, init);
        }
        self.at(b).sealed = true;
    }
}
