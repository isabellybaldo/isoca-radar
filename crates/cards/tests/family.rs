#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::G1Cell;
use cards::family::*;
use std::collections::{BTreeMap, BTreeSet};

fn run(id: i64, started: i64, end: Option<i64>, v: &[(&str, &str)]) -> RunPremises {
    RunPremises {
        span: RunSpan {
            run_id: id,
            started_ms: started,
            end_ms: end,
        },
        versions: v
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

const AREAS: &[&str] = &["episode_def", "fees"];

#[test]
fn the_current_key_is_the_newest_complete_run_and_only_its_twins_match() {
    let runs = vec![
        run(7, 0, Some(10), &[("episode_def", "1+a"), ("fees", "1+b")]),
        run(8, 20, Some(30), &[("episode_def", "2+a"), ("fees", "1+b")]),
        run(9, 40, Some(50), &[("episode_def", "1+a"), ("fees", "1+b")]),
        run(10, 60, None, &[("episode_def", "1+a")]), // missing fees: never counts
    ];
    let (key, matched) = current_key(AREAS, &runs).unwrap();
    assert_eq!(key, "episode_def=1+a;fees=1+b");
    assert_eq!(
        matched.iter().map(|r| r.run_id).collect::<Vec<_>>(),
        vec![7, 9],
        "run 8 has other premises"
    );
    assert_eq!(current_key(&["sim"], &runs), None);
}

#[test]
fn family_spans_hours_and_groups() {
    let mut f: Family<G1Cell> = Family::new(10 * H);
    f.runs.insert(
        3,
        RunSpan {
            run_id: 3,
            started_ms: 2 * H + 5,
            end_ms: Some(4 * H),
        },
    );
    f.runs.insert(
        4,
        RunSpan {
            run_id: 4,
            started_ms: 5 * H,
            end_ms: None,
        },
    );
    assert_eq!(f.since_ms(), Some(2 * H + 5));
    assert_eq!(f.run_ids(), vec![3, 4]);
    assert_eq!(f.run_end(3, 10 * H), 4 * H);
    assert_eq!(f.run_end(4, 10 * H), 10 * H);
    assert_eq!(f.run_end(99, 10 * H), 10 * H);
    assert_eq!(hours_between(H + 5, 3 * H), vec![H, 2 * H, 3 * H]);
    assert_eq!(hours_between(H + 5, H + 6), vec![H]);
    let set: BTreeSet<i64> = [0, H, 2 * H, 5 * H, 6 * H].into_iter().collect();
    assert_eq!(
        contiguous(&set, 2),
        vec![vec![0, H], vec![2 * H], vec![5 * H, 6 * H]]
    );
}
