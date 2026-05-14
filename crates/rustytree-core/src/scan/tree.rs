//! In-memory size tree built from scan results.
//!
//! Storage is a flat `Vec<Node>` arena indexed by [`NodeId`]. We never delete
//! nodes during a scan, so generation-counted slot maps would be overkill.
//!
//! Construction has two phases:
//! 1. The walker calls [`Tree::insert`] for each filesystem entry as it
//!    appears, supplying the parent [`NodeId`] (or `None` for the root).
//!    Each new node starts with `size_total == size_self` and zero counts.
//! 2. After the walk completes the caller invokes [`Tree::aggregate`] which
//!    bubbles `size_total`, `alloc_total`, `file_count`, and `dir_count` up
//!    from leaves to root in a single post-order pass.
//!
//! Children are kept in insertion order until [`Tree::sort_children_by_size`]
//! is called; the UI typically wants size-descending order at every level.

use std::time::SystemTime;

/// Opaque arena index into a [`Tree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Raw index into the underlying `Vec<Node>`. Useful for `egui` row IDs.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// What kind of filesystem entry a [`Node`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Dir,
    File,
    Symlink,
}

/// A single filesystem entry in the size tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// File or directory name (basename only). The root holds the original
    /// scan path's display string.
    pub name: String,
    pub kind: NodeKind,
    /// Logical bytes contributed by this entry alone (file length, or zero
    /// for directories and symlinks).
    pub size_self: u64,
    /// Logical bytes for this entry plus all descendants. Populated by
    /// [`Tree::aggregate`].
    pub size_total: u64,
    /// On-disk allocated bytes for this entry alone.
    pub alloc_self: u64,
    /// Allocated bytes for this entry plus all descendants.
    pub alloc_total: u64,
    /// Number of file descendants (does not count the node itself, even if
    /// it is a file). Populated by [`Tree::aggregate`].
    pub file_count: u64,
    /// Number of directory descendants (does not count the node itself).
    pub dir_count: u64,
    pub mtime: Option<SystemTime>,
    pub owner: Option<String>,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

impl Node {
    /// Build a leaf-style node (no children, totals == self values). The
    /// walker uses this and then attaches the node via [`Tree::insert`].
    pub fn new_leaf(
        name: impl Into<String>,
        kind: NodeKind,
        size_self: u64,
        alloc_self: u64,
        mtime: Option<SystemTime>,
        owner: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            size_self,
            size_total: size_self,
            alloc_self,
            alloc_total: alloc_self,
            file_count: 0,
            dir_count: 0,
            mtime,
            owner,
            parent: None,
            children: Vec::new(),
        }
    }
}

/// Arena-backed size tree.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    nodes: Vec<Node>,
    root: Option<NodeId>,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node to the arena. Pass `parent = None` to set the root (only
    /// allowed once; subsequent calls with `None` parent are rejected).
    ///
    /// # Panics
    ///
    /// Panics if the arena would grow past `u32::MAX` nodes (~4.29 B) —
    /// `NodeId` is internally a `u32` and silently truncating would alias
    /// every new node onto the existing root, corrupting the tree. In any
    /// realistic scan this is unreachable: a `Node` is well over 64 bytes,
    /// so an overflow would require >256 GB of in-memory tree state. The
    /// panic exists purely as a defensive fail-fast guard.
    pub fn insert(&mut self, parent: Option<NodeId>, mut node: Node) -> NodeId {
        let next_index = self.nodes.len();
        assert!(
            next_index < u32::MAX as usize,
            "Tree arena cannot exceed u32::MAX nodes (NodeId is a u32)"
        );
        let id = NodeId(next_index as u32);
        node.parent = parent;
        self.nodes.push(node);
        match parent {
            Some(p) => {
                self.nodes[p.0 as usize].children.push(id);
            }
            None => {
                debug_assert!(
                    self.root.is_none(),
                    "Tree::insert with None parent called more than once"
                );
                self.root = Some(id);
            }
        }
        id
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.0 as usize)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterate over every node id in the order it was inserted.
    pub fn iter_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        // Safe by construction: `Tree::insert` rejects insertions that
        // would push `nodes.len()` past `u32::MAX`, so the cast can never
        // truncate.
        (0..self.nodes.len() as u32).map(NodeId)
    }

    /// Walk all descendants of `start` (excluding `start` itself) in pre-order.
    pub fn descendants(&self, start: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.nodes[start.0 as usize]
            .children
            .iter()
            .rev()
            .copied()
            .collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            for child in self.nodes[id.0 as usize].children.iter().rev() {
                stack.push(*child);
            }
        }
        out
    }

    /// Bubble up totals and counts from leaves to root in a single post-order
    /// pass. Idempotent: calling twice produces the same result.
    pub fn aggregate(&mut self) {
        let Some(root) = self.root else {
            return;
        };

        // Build a post-order traversal iteratively to avoid recursion blowing
        // the stack on very deep trees.
        let mut order: Vec<NodeId> = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<(NodeId, bool)> = vec![(root, false)];
        while let Some((id, visited)) = stack.pop() {
            if visited {
                order.push(id);
                continue;
            }
            stack.push((id, true));
            for &child in self.nodes[id.0 as usize].children.iter().rev() {
                stack.push((child, false));
            }
        }

        for id in order {
            let idx = id.0 as usize;
            let mut size_total = self.nodes[idx].size_self;
            let mut alloc_total = self.nodes[idx].alloc_self;
            let mut file_count = 0u64;
            let mut dir_count = 0u64;
            // Snapshot child ids first so we can borrow `self.nodes` immutably
            // for each child without conflicting with the outer index.
            let child_ids: Vec<NodeId> = self.nodes[idx].children.clone();
            for child_id in child_ids {
                let child = &self.nodes[child_id.0 as usize];
                size_total = size_total.saturating_add(child.size_total);
                alloc_total = alloc_total.saturating_add(child.alloc_total);
                file_count = file_count.saturating_add(child.file_count);
                dir_count = dir_count.saturating_add(child.dir_count);
                match child.kind {
                    NodeKind::File | NodeKind::Symlink => file_count = file_count.saturating_add(1),
                    NodeKind::Dir => dir_count = dir_count.saturating_add(1),
                }
            }
            let n = &mut self.nodes[idx];
            n.size_total = size_total;
            n.alloc_total = alloc_total;
            n.file_count = file_count;
            n.dir_count = dir_count;
        }
    }

    /// Sort every node's children by `size_total` descending. Stable so
    /// equal-sized siblings keep insertion order (deterministic for tests).
    pub fn sort_children_by_size(&mut self) {
        for idx in 0..self.nodes.len() {
            // Snapshot total sizes so we don't borrow nodes while mutating.
            let mut children = std::mem::take(&mut self.nodes[idx].children);
            children.sort_by(|a, b| {
                let sa = self.nodes[a.0 as usize].size_total;
                let sb = self.nodes[b.0 as usize].size_total;
                sb.cmp(&sa)
            });
            self.nodes[idx].children = children;
        }
    }

    /// Fraction of `id`'s `size_total` over its parent's `size_total`. Returns
    /// `1.0` for the root and `0.0` if the parent is empty.
    pub fn percent_of_parent(&self, id: NodeId) -> f32 {
        let n = match self.get(id) {
            Some(n) => n,
            None => return 0.0,
        };
        let Some(parent_id) = n.parent else {
            return 1.0;
        };
        let parent_total = self.nodes[parent_id.0 as usize].size_total;
        if parent_total == 0 {
            0.0
        } else {
            n.size_total as f32 / parent_total as f32
        }
    }

    /// Fraction of `id`'s `size_total` over the root's `size_total`.
    pub fn percent_of_root(&self, id: NodeId) -> f32 {
        let Some(root) = self.root else { return 0.0 };
        let root_total = self.nodes[root.0 as usize].size_total;
        if root_total == 0 {
            return 0.0;
        }
        let n = match self.get(id) {
            Some(n) => n,
            None => return 0.0,
        };
        n.size_total as f32 / root_total as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> Node {
        Node::new_leaf(name, NodeKind::Dir, 0, 0, None, None)
    }

    fn file(name: &str, size: u64) -> Node {
        Node::new_leaf(name, NodeKind::File, size, size, None, None)
    }

    /// Build:
    ///   root (dir)
    ///     a (dir)
    ///       a1.bin 100
    ///       a2.bin 200
    ///     b (dir)
    ///       b1.bin  50
    ///     c.bin 1000
    fn sample_tree() -> Tree {
        let mut t = Tree::new();
        let root = t.insert(None, dir("root"));
        let a = t.insert(Some(root), dir("a"));
        t.insert(Some(a), file("a1.bin", 100));
        t.insert(Some(a), file("a2.bin", 200));
        let b = t.insert(Some(root), dir("b"));
        t.insert(Some(b), file("b1.bin", 50));
        t.insert(Some(root), file("c.bin", 1000));
        t
    }

    #[test]
    fn aggregate_bubbles_size_total_up() {
        let mut t = sample_tree();
        t.aggregate();
        let root = t.root().unwrap();
        assert_eq!(t.get(root).unwrap().size_total, 100 + 200 + 50 + 1000);

        let a = t.get(root).unwrap().children[0];
        assert_eq!(t.get(a).unwrap().size_total, 300);

        let b = t.get(root).unwrap().children[1];
        assert_eq!(t.get(b).unwrap().size_total, 50);
    }

    #[test]
    fn aggregate_counts_files_and_dirs_excluding_self() {
        let mut t = sample_tree();
        t.aggregate();
        let root = t.root().unwrap();
        let r = t.get(root).unwrap();
        assert_eq!(r.file_count, 4, "root has four file descendants");
        assert_eq!(r.dir_count, 2, "root has two directory descendants (a, b)");

        let a = r.children[0];
        let a_node = t.get(a).unwrap();
        assert_eq!(a_node.file_count, 2);
        assert_eq!(a_node.dir_count, 0);
    }

    #[test]
    fn aggregate_is_idempotent() {
        let mut t = sample_tree();
        t.aggregate();
        let snapshot: Vec<u64> = t
            .iter_ids()
            .map(|id| t.get(id).unwrap().size_total)
            .collect();
        t.aggregate();
        let after: Vec<u64> = t
            .iter_ids()
            .map(|id| t.get(id).unwrap().size_total)
            .collect();
        assert_eq!(snapshot, after);
    }

    #[test]
    fn sort_children_by_size_orders_descending() {
        let mut t = sample_tree();
        t.aggregate();
        t.sort_children_by_size();
        let root = t.root().unwrap();
        let kids = &t.get(root).unwrap().children;
        let totals: Vec<u64> = kids
            .iter()
            .map(|id| t.get(*id).unwrap().size_total)
            .collect();
        // Expect 1000 (c.bin), 300 (a), 50 (b)
        assert_eq!(totals, vec![1000, 300, 50]);
    }

    #[test]
    fn percent_of_root_sums_to_one_across_root_children() {
        let mut t = sample_tree();
        t.aggregate();
        let root = t.root().unwrap();
        let kids = t.get(root).unwrap().children.clone();
        let sum: f32 = kids.iter().map(|id| t.percent_of_root(*id)).sum();
        // Allow a tiny epsilon for f32 rounding.
        assert!((sum - 1.0).abs() < 1e-6, "got {sum}");
    }

    #[test]
    fn percent_of_parent_for_root_is_one() {
        let mut t = sample_tree();
        t.aggregate();
        let root = t.root().unwrap();
        assert_eq!(t.percent_of_parent(root), 1.0);
    }

    #[test]
    fn empty_tree_aggregate_is_noop() {
        let mut t = Tree::new();
        t.aggregate();
        assert!(t.is_empty());
    }

    #[test]
    fn descendants_excludes_start_node() {
        let mut t = sample_tree();
        t.aggregate();
        let root = t.root().unwrap();
        let desc = t.descendants(root);
        assert_eq!(desc.len(), 6, "5 file descendants + 2 dirs - root self = 6");
        assert!(!desc.contains(&root));
    }
}
