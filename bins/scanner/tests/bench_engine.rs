//! S3a-2 profiling (ignored by default). Engine µs/frame with the §8.2 threshold gate off (the
//! pre-S3a-2 behaviour: both detectors on every evaluation) and on, over one synthetic
//! high-rate script. Engine time only: frames are pre-built, and outputs are hashed after the clock stops.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;
use scanner::engine::Out;
use sha2::{Digest, Sha256};
use std::time::Instant;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "profiling: run by hand (S3a-2 plan, Task 3)"]
fn engine_us_per_frame_with_and_without_the_threshold_gate() {
    let groups = env_usize("BENCH_GROUPS", 2_000);
    let n = env_usize("BENCH_FRAMES", 100_000);
    let mut s = Synth::new(groups, 7);
    let frames = s.script(n, 0); // 0: the bench's own script never carries a wrong top
    let mut digests: Vec<Vec<u8>> = Vec::new();
    for gate in [false, true] {
        let mut e = synth_engine(&s, gate);
        let mut outs: Vec<Out> = Vec::with_capacity(frames.len() * 2);
        let t = Instant::now();
        drive(&mut e, &frames, 0, &mut |o| outs.push(o));
        let elapsed = t.elapsed();
        let ns = elapsed.as_nanos() / u128::try_from(frames.len()).unwrap();
        let episode_rows = outs.iter().filter(|o| matches!(o, Out::Episode(_))).count();
        let mut h = Sha256::new();
        for o in &outs {
            h.update(format!("{o:?}").as_bytes());
        }
        digests.push(h.finalize().to_vec());
        println!(
            "gate={gate:<5} groups={groups} frames={} engine={}.{:03} µs/frame detector_calls={} episode_rows={episode_rows}",
            frames.len(),
            ns / 1_000,
            ns % 1_000,
            e.detector_calls(),
        );
    }
    println!("VPS budget at ~4.3k frames/s: ~232 µs/frame on one core, shared with parse_frame");
    assert_eq!(digests[0], digests[1], "the gate changed the outputs");
}
