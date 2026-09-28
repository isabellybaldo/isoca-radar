#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::{day_dir, part_path};
use compact::hour::process_hour;
use compact::pq::read_all;
use compact::reduce::Reducer;
use compact::rows::L2Row;
use compact::state;
use fixture::*;

#[test]
fn resume_from_a_checkpoint_after_hour_1_matches_a_straight_run_byte_for_byte() {
    let raw = tmp("resume-raw");
    three_hours(&raw);
    let (a, b) = (tmp("resume-a"), tmp("resume-b"));
    let mut straight = Reducer::new(rcfg());
    for i in 0..3 {
        process_hour(&mut straight, &hour_job(&raw, &a, i))
            .unwrap()
            .unwrap();
    }
    let mut first = Reducer::new(rcfg());
    let rep = process_hour(&mut first, &hour_job(&raw, &b, 0))
        .unwrap()
        .unwrap();
    state::save(
        &b,
        &state::Checkpoint {
            hour_ms: T0,
            report: rep.clone(),
            reducer: first.state(),
        },
    )
    .unwrap();
    drop(first);
    let cp = state::load_newest(&b).unwrap().unwrap();
    assert_eq!(cp.hour_ms, T0);
    assert_eq!(
        cp.report, rep,
        "the checkpointed report round-trips exactly"
    );
    let mut resumed = Reducer::from_state(rcfg(), cp.reducer).unwrap();
    for i in 1..3 {
        process_hour(&mut resumed, &hour_job(&raw, &b, i))
            .unwrap()
            .unwrap();
    }
    let (fa, fb) = (day_files(&a), day_files(&b));
    assert_eq!(fa.len(), 18, "3 hours × (5 parts + sidecar)");
    assert_eq!(fa, fb, "every part and sidecar identical, byte for byte");
    let l2 = read_all::<L2Row>(&part_path(&day_dir(&a, T0), 1, "l2")).unwrap();
    assert!(
        !l2.is_empty() && l2[0].snapshot,
        "hour 1 has the window, starting with snapshots"
    );
    for d in [&raw, &a, &b] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn checkpoints_keep_the_newest_two_and_skip_a_corrupt_one() {
    let (raw, arch) = (tmp("cp-raw"), tmp("cp-arch"));
    three_hours(&raw);
    let mut red = Reducer::new(rcfg());
    for i in 0..3 {
        let rep = process_hour(&mut red, &hour_job(&raw, &arch, i))
            .unwrap()
            .unwrap();
        state::save(
            &arch,
            &state::Checkpoint {
                hour_ms: T0 + i * H,
                report: rep,
                reducer: red.state(),
            },
        )
        .unwrap();
    }
    let l = state::list(&arch);
    assert_eq!(
        l.iter().map(|(h, _)| *h).collect::<Vec<_>>(),
        vec![T0 + H, T0 + 2 * H]
    );
    std::fs::write(&l[1].1, b"not zstd").unwrap();
    assert_eq!(
        state::load_newest(&arch).unwrap().unwrap().hour_ms,
        T0 + H,
        "a corrupt newest falls back to the older one"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn load_newest_is_none_only_when_no_checkpoint_file_exists() {
    let arch = tmp("cp-empty");
    assert_eq!(state::load_newest(&arch).unwrap(), None);
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn load_newest_errs_when_every_checkpoint_is_corrupt() {
    let (raw, arch) = (tmp("cp-allcorrupt-raw"), tmp("cp-allcorrupt-arch"));
    three_hours(&raw);
    let mut red = Reducer::new(rcfg());
    for i in 0..2 {
        let rep = process_hour(&mut red, &hour_job(&raw, &arch, i))
            .unwrap()
            .unwrap();
        state::save(
            &arch,
            &state::Checkpoint {
                hour_ms: T0 + i * H,
                report: rep,
                reducer: red.state(),
            },
        )
        .unwrap();
    }
    let l = state::list(&arch);
    assert_eq!(l.len(), 2, "both checkpoints kept (fewer than KEEP so far)");
    for (_, p) in &l {
        std::fs::write(p, b"not zstd").unwrap();
    }
    let err = state::load_newest(&arch).unwrap_err();
    let msg = format!("{err:#}");
    for (_, p) in &l {
        assert!(
            msg.contains(&p.display().to_string()),
            "error names {}: {msg}",
            p.display()
        );
    }
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}
