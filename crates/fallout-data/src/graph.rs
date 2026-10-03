//! Shared structural graph traversal for bounded source dependency plans.
// Iterative Kosaraju keeps traversal memory proportional to the bounded graph.
// A deeply nested authored list cannot consume the native call stack.
pub(crate) fn cyclic_components(children: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut reverse = vec![Vec::new(); children.len()];
    for (from, targets) in children.iter().enumerate() {
        for &to in targets {
            reverse[to].push(from);
        }
    }
    let mut seen = vec![false; children.len()];
    let mut order = Vec::with_capacity(children.len());
    for root in 0..children.len() {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut stack = vec![(root, 0)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < children[*node].len() {
                let to = children[*node][*next];
                *next += 1;
                if !seen[to] {
                    seen[to] = true;
                    stack.push((to, 0));
                }
            } else {
                let (node, _) = stack.pop().expect("active graph frame");
                order.push(node);
            }
        }
    }
    seen.fill(false);
    let mut result = Vec::new();
    for &root in order.iter().rev() {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut stack = vec![root];
        let mut component = Vec::new();
        while let Some(node) = stack.pop() {
            component.push(node);
            for &to in &reverse[node] {
                if !seen[to] {
                    seen[to] = true;
                    stack.push(to);
                }
            }
        }
        component.sort_unstable();
        if component.len() > 1 || children[root].contains(&root) {
            result.push(component);
        }
    }
    result.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycles_distinguish_shared_children_self_links_and_disconnected_components() {
        let graph = vec![
            vec![1, 1, 3],
            vec![2],
            vec![1, 3],
            vec![],
            vec![4],
            vec![6],
            vec![],
        ];
        assert_eq!(cyclic_components(&graph), vec![vec![1, 2], vec![4]]);
    }
    #[test]
    fn very_deep_graph_uses_no_recursive_call_stack() {
        let mut graph = (0..100_000)
            .map(|i| {
                if i + 1 < 100_000 {
                    vec![i + 1]
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>();
        assert!(cyclic_components(&graph).is_empty());
        graph[99_999].push(0);
        let cycles = cyclic_components(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 100_000);
    }
}
