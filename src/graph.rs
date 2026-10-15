//! Graph construction, BFS reachability/depth, orphans and hierarchy checks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Directed edge source -> target (page keys).
pub struct Graph {
    pub edges: BTreeMap<String, BTreeSet<String>>,
    pub nodes: BTreeSet<String>,
}

impl Graph {
    pub fn new() -> Graph {
        Graph {
            edges: BTreeMap::new(),
            nodes: BTreeSet::new(),
        }
    }

    pub fn add_node(&mut self, n: impl Into<String>) {
        self.nodes.insert(n.into());
    }

    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.nodes.insert(from.to_string());
        self.nodes.insert(to.to_string());
        self.edges
            .entry(from.to_string())
            .or_default()
            .insert(to.to_string());
    }

    /// BFS from entrypoints over directed edges. Returns (depth per node,
    /// reachable set). Self-loops and same-page references do not create
    /// inbound edges (filtered by caller), so they cannot rescue orphans.
    pub fn bfs(&self, entrypoints: &[String]) -> (BTreeMap<String, usize>, BTreeSet<String>) {
        let mut depth: BTreeMap<String, usize> = BTreeMap::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        for e in entrypoints {
            if self.nodes.contains(e) && !depth.contains_key(e) {
                depth.insert(e.clone(), 0);
                queue.push_back(e.clone());
            }
        }
        while let Some(n) = queue.pop_front() {
            let d = depth[&n];
            if let Some(out) = self.edges.get(&n) {
                for t in out {
                    if !depth.contains_key(t) {
                        depth.insert(t.clone(), d + 1);
                        queue.push_back(t.clone());
                    }
                }
            }
        }
        let reachable: BTreeSet<String> = depth.keys().cloned().collect();
        (depth, reachable)
    }

    /// Orphan nodes: no inbound edge from a DIFFERENT node.
    pub fn orphans(&self) -> BTreeSet<String> {
        let mut inbound: BTreeSet<String> = BTreeSet::new();
        for (from, outs) in &self.edges {
            for t in outs {
                if t != from {
                    inbound.insert(t.clone());
                }
            }
        }
        self.nodes
            .iter()
            .filter(|n| !inbound.contains(*n))
            .cloned()
            .collect()
    }

    /// Nodes not reachable from any entrypoint.
    pub fn unreachable(&self, entrypoints: &[String]) -> BTreeSet<String> {
        let (_, reachable) = self.bfs(entrypoints);
        self.nodes.difference(&reachable).cloned().collect()
    }

    /// Nodes in cycles: strongly connected components of size > 1, plus
    /// self-loop nodes. Tarjan-free approach: a node is cyclic if it can
    /// reach itself via >= 1 edge. Computed with BFS per node (graphs are
    /// site-scale, at most a few thousand nodes).
    pub fn cyclic_nodes(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for n in &self.nodes {
            if self.edges.get(n).map(|s| s.contains(n)).unwrap_or(false) {
                out.insert(n.clone());
                continue;
            }
            if self.reaches(n, n) {
                out.insert(n.clone());
            }
        }
        out
    }

    fn reaches(&self, from: &str, to: &str) -> bool {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        if let Some(outs) = self.edges.get(from) {
            for t in outs {
                seen.insert(t.clone());
                queue.push_back(t.clone());
            }
        }
        while let Some(n) = queue.pop_front() {
            if n == to {
                return true;
            }
            if let Some(outs) = self.edges.get(&n) {
                for t in outs {
                    if seen.insert(t.clone()) {
                        queue.push_back(t.clone());
                    }
                }
            }
        }
        false
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

/// Cycle detection for an explicit child->parent mapping.
/// Returns (nodes in cycles, children with missing parents).
pub fn hierarchy_problems(
    child_to_parent: &BTreeMap<String, String>,
    known_nodes: &BTreeSet<String>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut cycles: BTreeSet<String> = BTreeSet::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for (child, parent) in child_to_parent {
        if !known_nodes.contains(parent) {
            missing.insert(child.clone());
        }
    }
    for start in child_to_parent.keys() {
        // Walk up the parent chain; a node is IN a cycle only when the walk
        // returns to the node itself. A node whose chain merely reaches a
        // cycle (tail into it) is not a cycle member.
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut cur = Some(start.clone());
        while let Some(c) = cur {
            if !seen.insert(c.clone()) {
                if c == *start {
                    cycles.insert(start.clone());
                }
                break;
            }
            cur = child_to_parent.get(&c).cloned();
        }
    }
    (cycles, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_into_cycle_is_not_a_cycle_member() {
        let mut parents: BTreeMap<String, String> = BTreeMap::new();
        parents.insert("/tail".to_string(), "/b".to_string());
        parents.insert("/b".to_string(), "/c".to_string());
        parents.insert("/c".to_string(), "/b".to_string());
        let mut known: BTreeSet<String> = BTreeSet::new();
        for k in ["/tail", "/b", "/c"] {
            known.insert(k.to_string());
        }
        let (cycles, missing) = hierarchy_problems(&parents, &known);
        assert!(cycles.contains("/b"));
        assert!(cycles.contains("/c"));
        assert!(!cycles.contains("/tail"), "tail must not be a cycle member");
        assert!(missing.is_empty());
    }

    fn g() -> Graph {
        let mut gr = Graph::new();
        gr.add_edge("/", "/about/");
        gr.add_edge("/about/", "/contact/");
        gr
    }

    #[test]
    fn bfs_depths() {
        let gr = g();
        let (depth, reachable) = gr.bfs(&["/".to_string()]);
        assert_eq!(depth["/"], 0);
        assert_eq!(depth["/about/"], 1);
        assert_eq!(depth["/contact/"], 2);
        assert!(reachable.contains("/contact/"));
    }

    #[test]
    fn self_links_do_not_rescue_orphans() {
        let mut gr = Graph::new();
        gr.add_node("/lonely/");
        gr.add_edge("/lonely/", "/lonely/");
        gr.add_edge("/", "/other/");
        gr.add_node("/other/");
        assert!(gr.orphans().contains("/lonely/"));
        assert!(!gr.orphans().contains("/other/"));
    }

    #[test]
    fn shortest_depth_not_first_found() {
        let mut gr = Graph::new();
        gr.add_edge("/", "/a/");
        gr.add_edge("/a/", "/b/");
        gr.add_edge("/a/", "/c/");
        gr.add_edge("/c/", "/b/");
        gr.add_node("/b/");
        let (depth, _) = gr.bfs(&["/".to_string()]);
        assert_eq!(depth["/b/"], 2);
    }

    #[test]
    fn unreachable_and_cycles() {
        let mut gr = Graph::new();
        gr.add_edge("/", "/a/");
        gr.add_node("/a/");
        gr.add_edge("/x/", "/y/");
        gr.add_edge("/y/", "/x/");
        gr.add_node("/y/");
        let un = gr.unreachable(&["/".to_string()]);
        assert!(un.contains("/x/"));
        assert!(un.contains("/y/"));
        let cyc = gr.cyclic_nodes();
        assert!(cyc.contains("/x/"));
        assert!(cyc.contains("/y/"));
        assert!(!cyc.contains("/"));
    }

    #[test]
    fn hierarchy_cycles_and_missing() {
        let mut m = BTreeMap::new();
        m.insert("/a/".to_string(), "/b/".to_string());
        m.insert("/b/".to_string(), "/a/".to_string());
        m.insert("/c/".to_string(), "/ghost/".to_string());
        let known: BTreeSet<String> = ["/a/", "/b/", "/c/"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (cycles, missing) = hierarchy_problems(&m, &known);
        assert!(cycles.contains("/a/"));
        assert!(cycles.contains("/b/"));
        assert!(missing.contains("/c/"));
    }
}
