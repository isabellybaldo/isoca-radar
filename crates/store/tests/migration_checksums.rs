//! The checksums production has recorded in `_sqlx_migrations`. sqlx refuses to start when an
//! applied migration's embedded bytes change, and a changed line ending counts (2026-09-27: the
//! 5d deploy failed twice on CRLF copies of 0001 and 0006). This test fails on the PC instead of on
//! the VPS. An applied migration is never edited; a new one adds its line here once deployed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

const APPLIED: &[(i64, &str)] = &[
    (
        1,
        "c5e8b5dda8c24f106ba88a809dd1579c129b35f6ec96be7374744539746b95171d6390412b59125c2646202f018fbfd4",
    ),
    (
        2,
        "a221cfe3ef3b5a8e610f90e5ee19442197eed8bf7b18acab720a791ff12f6f9b298fb08da30b6c2b58d60fd562f0e15c",
    ),
    (
        3,
        "88fdf87a47cb65cbee3648cff1e4ea04ad26800ea6dfd38fd036dfcff4dedda4f18196321904dd0af91d9ef1ef018467",
    ),
    (
        4,
        "6dd8255b5d8fc859503f17b1cb23fa9597c6d633f3d40ec0f4f2ef9d487960102fbdf88258d9841fba6bdae28df08c66",
    ),
    (
        5,
        "5d54043c67e525ffafbf314b7ff2f0426804da85eee5202c0ab369d4007451a67eaf170da9104a4ba00045e38a7d5e8d",
    ),
    (
        6,
        "5aa97de18927be0af3850fb47ebda9496a64ed94426b266ac946a762865d90ea6071125d946abd33ae16760c9000595b",
    ),
    (
        7,
        "6450cd23d19340bdfd635ab1a06ad8a2be4eb90883fc2b34753c209e5a669daadda415348e98c886edca897fed772c5e",
    ),
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn applied_migrations_keep_the_checksums_production_recorded() {
    for &(version, want) in APPLIED {
        let m = store::MIGRATOR
            .iter()
            .find(|m| m.version == version)
            .unwrap_or_else(|| panic!("migration {version} is missing"));
        assert_eq!(
            hex(&m.checksum),
            want,
            "migration {version} changed bytes (a CRLF checkout? an edit?): production will refuse it"
        );
    }
}

#[test]
fn every_embedded_migration_is_pinned_or_newer_than_production() {
    let newest = APPLIED.iter().map(|(v, _)| *v).max().unwrap();
    for m in store::MIGRATOR.iter() {
        assert!(
            m.version > newest || APPLIED.iter().any(|(v, _)| *v == m.version),
            "migration {} is neither pinned nor newer than {newest}",
            m.version
        );
    }
}
