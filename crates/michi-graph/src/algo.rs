//! Index-level algorithms: adjacency, Kahn, Kosaraju and shortest cycles. Nothing here recurses.

use alloc::collections::BinaryHeap;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Reverse;

use crate::NodeIx;

/// Compressed adjacency: the neighbours of node `i` are `targets[offsets[i]..offsets[i + 1]]`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Csr {
    offsets: Vec<u32>,
    targets: Vec<NodeIx>,
}

impl Csr {
    /// Builds from `(node, neighbour)` pairs over `n` nodes, dropping repeats and listing each node's neighbours by
    /// `rank`.
    pub(crate) fn from_pairs(n: usize, mut pairs: Vec<(u32, u32)>, rank: &[u32]) -> Self {
        let r = |i: u32| rank.get(i as usize).copied().unwrap_or(u32::MAX);
        pairs.sort_unstable_by_key(|&(a, b)| (a, r(b)));
        pairs.dedup();
        let mut offsets = vec![0u32; n + 1];
        for &(a, _) in &pairs {
            if let Some(o) = offsets.get_mut(a as usize + 1) {
                *o += 1;
            }
        }
        for i in 1..offsets.len() {
            let prev = offsets.get(i - 1).copied().unwrap_or(0);
            if let Some(o) = offsets.get_mut(i) {
                *o += prev;
            }
        }
        Self { offsets, targets: pairs.into_iter().map(|(_, b)| NodeIx(b)).collect() }
    }

    pub(crate) fn of(&self, i: usize) -> &[NodeIx] {
        let (Some(&start), Some(&end)) = (self.offsets.get(i), self.offsets.get(i + 1)) else { return &[] };
        self.targets.get(start as usize..end as usize).unwrap_or(&[])
    }
}

/// Kahn's algorithm, always taking the ready node of lowest rank: the lexicographically smallest topological order.
/// Returns the order, which is shorter than `n` when some nodes lie on or behind a cycle.
pub(crate) fn kahn(n: usize, prerequisites: &Csr, dependents: &Csr, rank: &[u32], by_rank: &[u32]) -> Vec<NodeIx> {
    let mut indegree: Vec<u32> = (0..n).map(|i| u32::try_from(prerequisites.of(i).len()).unwrap_or(u32::MAX)).collect();
    let mut heap: BinaryHeap<Reverse<u32>> =
        (0..n).filter(|&i| indegree.get(i) == Some(&0)).filter_map(|i| rank.get(i).copied()).map(Reverse).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(Reverse(r)) = heap.pop() {
        let Some(&i) = by_rank.get(r as usize) else { continue };
        order.push(NodeIx(i));
        for d in dependents.of(i as usize) {
            if let Some(deg) = indegree.get_mut(d.u()) {
                *deg -= 1;
                if *deg == 0 {
                    if let Some(&rd) = rank.get(d.u()) {
                        heap.push(Reverse(rd));
                    }
                }
            }
        }
    }
    order
}

/// Strongly connected components of the subgraph induced by `members` (Kosaraju, iterative). Only components that
/// form a cycle are returned: two or more nodes, or one node with an edge to itself (`self_loop`).
pub(crate) fn cyclic_components(
    members: &[bool],
    forward: &Csr,
    backward: &Csr,
    self_loop: &[bool],
) -> Vec<Vec<NodeIx>> {
    let n = members.len();
    let inside = |i: usize| members.get(i).copied().unwrap_or(false);

    // Pass 1: finish order of a depth-first search over `forward`.
    let mut visited = vec![false; n];
    let mut finished: Vec<usize> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for root in 0..n {
        if !inside(root) || visited.get(root).copied().unwrap_or(true) {
            continue;
        }
        mark(&mut visited, root);
        stack.push((root, 0));
        while let Some(top) = stack.last_mut() {
            let (node, next) = *top;
            match forward.of(node).get(next) {
                Some(child) => {
                    top.1 += 1;
                    let c = child.u();
                    if inside(c) && !visited.get(c).copied().unwrap_or(true) {
                        mark(&mut visited, c);
                        stack.push((c, 0));
                    }
                }
                None => {
                    finished.push(node);
                    stack.pop();
                }
            }
        }
    }

    // Pass 2: in reverse finish order, everything reachable over `backward` is one component.
    let mut assigned = vec![false; n];
    let mut components = Vec::new();
    let mut frontier: Vec<usize> = Vec::new();
    for &root in finished.iter().rev() {
        if assigned.get(root).copied().unwrap_or(true) {
            continue;
        }
        mark(&mut assigned, root);
        let mut component = vec![NodeIx(root as u32)];
        frontier.push(root);
        while let Some(node) = frontier.pop() {
            for p in backward.of(node) {
                let i = p.u();
                if inside(i) && !assigned.get(i).copied().unwrap_or(true) {
                    mark(&mut assigned, i);
                    component.push(*p);
                    frontier.push(i);
                }
            }
        }
        let cyclic = component.len() > 1 || self_loop.get(root).copied().unwrap_or(false);
        if cyclic {
            components.push(component);
        }
    }
    components
}

fn mark(flags: &mut [bool], i: usize) {
    if let Some(f) = flags.get_mut(i) {
        *f = true;
    }
}

/// Scratch space for [`shortest_cycle`], allocated once per report and reset only where a search touched it, so
/// reporting many small cycles stays linear in the graph rather than quadratic.
pub(crate) struct CycleScratch {
    parent: Vec<u32>,
    touched: Vec<usize>,
    queue: alloc::collections::VecDeque<usize>,
}

const UNSEEN: u32 = u32::MAX;

impl CycleScratch {
    pub(crate) fn new(n: usize) -> Self {
        Self { parent: vec![UNSEEN; n], touched: Vec::new(), queue: alloc::collections::VecDeque::new() }
    }
}

/// One shortest closed path through `start` that stays inside `component`, following `forward` edges. The path
/// lists `start` first and does not repeat it at the end. Work is proportional to the component searched.
pub(crate) fn shortest_cycle(
    start: NodeIx,
    component: &[bool],
    forward: &Csr,
    scratch: &mut CycleScratch,
) -> Vec<NodeIx> {
    let s = start.u();
    let path = search(s, component, forward, scratch).unwrap_or_else(|| vec![start]);
    for &t in &scratch.touched {
        if let Some(p) = scratch.parent.get_mut(t) {
            *p = UNSEEN;
        }
    }
    scratch.touched.clear();
    scratch.queue.clear();
    path
}

fn search(s: usize, component: &[bool], forward: &Csr, scratch: &mut CycleScratch) -> Option<Vec<NodeIx>> {
    let CycleScratch { parent, touched, queue } = scratch;
    *parent.get_mut(s)? = s as u32;
    touched.push(s);
    queue.push_back(s);
    while let Some(node) = queue.pop_front() {
        for next in forward.of(node) {
            let i = next.u();
            if i == s {
                let mut path = vec![NodeIx(node as u32)];
                let mut at = node;
                while at != s {
                    let p = *parent.get(at)?;
                    path.push(NodeIx(p));
                    at = p as usize;
                }
                path.reverse();
                return Some(path);
            }
            if component.get(i).copied().unwrap_or(false) && parent.get(i) == Some(&UNSEEN) {
                *parent.get_mut(i)? = node as u32;
                touched.push(i);
                queue.push_back(i);
            }
        }
    }
    None
}
