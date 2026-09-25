//! Event-affine shard packing (spec §7): first-fit decreasing; a group is never split.
use std::cmp::Reverse;

pub fn pack_shards(groups: &[Vec<String>], max_tokens: usize) -> Vec<Vec<String>> {
    let mut sorted: Vec<&Vec<String>> = groups.iter().collect();
    sorted.sort_by_key(|a| Reverse(a.len()));
    let mut shards: Vec<Vec<String>> = Vec::new();
    for g in sorted {
        match shards.iter_mut().find(|s| s.len() + g.len() <= max_tokens) {
            Some(s) => s.extend(g.iter().cloned()),
            None => shards.push(g.clone()),
        }
    }
    shards
}

#[cfg(test)]
mod tests {
    use super::*;
    fn g(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    }

    #[test]
    fn never_splits_a_group_and_respects_the_limit() {
        let groups = vec![g("a", 3), g("b", 2), g("c", 2), g("d", 7)];
        let shards = pack_shards(&groups, 5);
        // d (7 > 5) gets its own over-size shard; the rest fit in ≤ 5
        assert!(shards.iter().any(|s| s.len() == 7));
        for s in &shards {
            assert!(s.len() <= 5 || s.len() == 7);
        }
        for grp in &groups {
            let homes = shards.iter().filter(|s| s.contains(&grp[0])).count();
            assert_eq!(homes, 1);
            let home = shards.iter().find(|s| s.contains(&grp[0])).unwrap();
            assert!(
                grp.iter().all(|t| home.contains(t)),
                "group split across shards"
            );
        }
        assert_eq!(shards.iter().map(Vec::len).sum::<usize>(), 14);
    }
}
