#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The compactor reads universe records through tape's view; it must read what the scanner writes.
use rust_decimal::Decimal;
use scanner::capture::{UniverseRecord, universe_file_name};
use scanner::universe::{CanaryPair, ConnMap, GroupMeta, LegMeta, Status, Universe};
use tape::universe_view::RecordView;

const T0: i64 = 1_790_121_600_000;

fn leg(t: &str, status: Status) -> LegMeta {
    LegMeta {
        market_id: format!("m-{t}"),
        yes_token: t.into(),
        no_token: format!("n-{t}"),
        status,
        fee_rate: Decimal::ZERO,
        fee_exponent: 1,
        tick: None,
        min_size: None,
        seconds_delay: None,
        end_date: None,
    }
}

#[test]
fn the_record_view_reads_what_the_scanner_writes() {
    let u = Universe {
        taken_ms: T0,
        groups: vec![GroupMeta {
            event_id: "e1".into(),
            slug: None,
            title: None,
            neg_risk_market_id: Some("nr-e1".into()),
            augmented: false,
            legs: vec![leg("a1", Status::Live), leg("a3", Status::Halted)],
            tags: Some(vec!["politics".into()]),
        }],
        canary: vec![CanaryPair {
            market_id: "m9".into(),
            event_id: "e9".into(),
            yes_token: "cy".into(),
            no_token: "cn".into(),
        }],
    };
    let rec = UniverseRecord {
        universe: u,
        conn_of: [("e1".to_string(), 0), ("canary:m9".to_string(), 1)]
            .into_iter()
            .collect::<ConnMap>(),
        file: universe_file_name(T0, 3),
    };
    let v: RecordView = serde_json::from_slice(&serde_json::to_vec(&rec).unwrap()).unwrap();
    assert_eq!(v.file, rec.file);
    assert_eq!(v.universe.taken_ms, T0);
    assert_eq!(v.conn_of.get("canary:m9"), Some(&1));
    assert_eq!(
        v.universe.groups[0].neg_risk_market_id.as_deref(),
        Some("nr-e1")
    );
    let legs: Vec<(String, String)> = v.universe.groups[0]
        .legs
        .iter()
        .map(|l| (l.yes_token.clone(), l.status.clone()))
        .collect();
    assert_eq!(
        legs,
        vec![("a1".into(), "live".into()), ("a3".into(), "halted".into())]
    );
    assert_eq!(v.universe.canary[0].no_token, "cn");
    assert_eq!(scanner::shards::canary_key("m9"), "canary:m9");
}
