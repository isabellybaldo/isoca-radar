#![allow(clippy::unwrap_used, clippy::expect_used)]
use pm_api::backoff::{Backoff, ConnectBudget};

#[test]
fn backoff_doubles_to_the_cap() {
    let mut b = Backoff::new(1_000, 60_000, 30_000);
    let delays: Vec<u64> = (0..8).map(|i| b.on_disconnected(i)).collect();
    assert_eq!(
        delays,
        vec![1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000]
    );
}

#[test]
fn backoff_resets_only_after_a_healthy_session() {
    let mut b = Backoff::new(1_000, 60_000, 30_000);
    b.on_disconnected(0);
    b.on_disconnected(1);
    b.on_connected(10_000);
    // flapping: dropped after 5 s → keeps growing
    assert_eq!(b.on_disconnected(15_000), 4_000);
    b.on_connected(20_000);
    // healthy: dropped after 31 s → back to base
    assert_eq!(b.on_disconnected(51_000), 1_000);
}

#[test]
fn budget_limits_per_second_and_per_minute() {
    let mut g = ConnectBudget::new(2, 20);
    assert!(g.try_take(0));
    assert!(g.try_take(10));
    assert!(
        !g.try_take(20),
        "third connect in the same second is refused"
    );
    assert!(g.try_take(1_000));
    let mut taken = 3;
    let mut t = 2_000;
    while t < 60_000 {
        if g.try_take(t) {
            taken += 1;
        }
        t += 500;
    }
    assert_eq!(taken, 20, "never more than 20 in any minute");
    assert!(g.try_take(61_000), "the window slides");
}
