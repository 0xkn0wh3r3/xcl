//! Basic blocks of a decoded RDNA2 program, their edges and a reverse
//! post-order (the emission order: every block after its dominators).

use std::collections::{BTreeMap, BTreeSet};

use crate::gpu::rdna2::{self, Decoded};

/// How a block ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    /// falls through / jumps to the block (index)
    Jump(usize),
    /// conditional branch: (instruction index of the branch, taken, not taken)
    Cond(usize, usize, usize),
    /// s_endpgm or the end of the code
    End,
}

pub struct CfgBlock {
    /// instruction indices (the terminating branch excluded for `Cond`/`Jump` by branch)
    pub insts: Vec<usize>,
    pub term: Term,
    pub preds: Vec<usize>,
}

pub struct Cfg {
    pub insts: Vec<Decoded>,
    pub blocks: Vec<CfgBlock>,
    /// reachable blocks in reverse post-order (blocks[rpo[0]] is the entry)
    pub rpo: Vec<usize>,
    /// targets of back edges (loop headers)
    pub headers: BTreeSet<usize>,
}

fn is_end(name: &str) -> bool {
    matches!(name, "S_ENDPGM" | "S_ENDPGM_SAVED" | "S_ENDPGM_ORDERED_PS_DONE")
}

impl Cfg {
    pub fn build(code: &[u8]) -> Result<Cfg, String> {
        let mut insts: Vec<Decoded> = Vec::new();
        let mut off = 0usize;
        while off + 4 <= code.len() {
            let d = rdna2::decode(&code[off..], off as u64).map_err(|e| format!("+{off:#x}: cannot decode ({e:?})"))?;
            off += d.len;
            insts.push(d);
        }
        rdna2::resolve_far_jumps(&mut insts);
        let index: BTreeMap<u64, usize> = insts.iter().enumerate().map(|(i, d)| (d.pc, i)).collect();
        let mut leaders: BTreeSet<usize> = BTreeSet::from([0]);
        for (i, d) in insts.iter().enumerate() {
            if let Some(t) = rdna2::branch_target(d) {
                let ti = *index.get(&t).ok_or_else(|| format!("+{:#x}: branch to {t:#x}, which is not the start of an instruction", d.pc))?;
                leaders.insert(ti);
                leaders.insert(i + 1);
            } else if d.inst.flags & rdna2::F_TERMINATOR != 0 || is_end(d.inst.name) {
                leaders.insert(i + 1);
            }
        }
        let starts: Vec<usize> = leaders.into_iter().filter(|&l| l < insts.len()).collect();
        let block_of: BTreeMap<usize, usize> = starts.iter().enumerate().map(|(b, &i)| (i, b)).collect();
        let mut blocks = Vec::new();
        for (b, &s) in starts.iter().enumerate() {
            let e = starts.get(b + 1).copied().unwrap_or(insts.len());
            let mut list = Vec::new();
            let mut term = if e < insts.len() { Term::Jump(b + 1) } else { Term::End };
            for i in s..e {
                let d = &insts[i];
                if is_end(d.inst.name) {
                    term = Term::End;
                    break;
                }
                if let Some(t) = rdna2::branch_target(d) {
                    let tb = block_of[&index[&t]];
                    let name = d.inst.name;
                    term = if name == "S_BRANCH" || d.far_target.is_some() {
                        Term::Jump(tb)
                    } else {
                        let next = if e < insts.len() { b + 1 } else { usize::MAX };
                        if next == usize::MAX {
                            return Err(format!("+{:#x}: conditional branch at the end of the code", d.pc));
                        }
                        Term::Cond(i, tb, next)
                    };
                    break;
                }
                list.push(i);
            }
            blocks.push(CfgBlock { insts: list, term, preds: Vec::new() });
        }
        // a conditional branch whose both edges meet is a jump
        for b in &mut blocks {
            if let Term::Cond(_, t, f) = b.term {
                if t == f {
                    b.term = Term::Jump(t);
                }
            }
        }
        // reverse post-order from the entry (iterative DFS)
        let n = blocks.len();
        let succ = |b: &CfgBlock| -> Vec<usize> {
            match b.term {
                Term::Jump(t) => vec![t],
                Term::Cond(_, t, f) => vec![t, f],
                Term::End => vec![],
            }
        };
        let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
        let mut post = Vec::new();
        let mut headers = BTreeSet::new();
        if n > 0 {
            let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
            state[0] = 1;
            while let Some(&mut (b, ref mut k)) = stack.last_mut() {
                let s = succ(&blocks[b]);
                if *k < s.len() {
                    let t = s[*k];
                    *k += 1;
                    match state[t] {
                        0 => {
                            state[t] = 1;
                            stack.push((t, 0));
                        }
                        1 => {
                            headers.insert(t);
                        }
                        _ => {}
                    }
                } else {
                    state[b] = 2;
                    post.push(b);
                    stack.pop();
                }
            }
        }
        post.reverse();
        let reach: BTreeSet<usize> = post.iter().copied().collect();
        for &b in &post {
            for t in succ(&blocks[b]) {
                if !blocks[t].preds.contains(&b) {
                    blocks[t].preds.push(b);
                }
            }
        }
        let _ = reach;
        Ok(Cfg { insts, blocks, rpo: post, headers })
    }
}
