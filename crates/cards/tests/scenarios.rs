#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
fn the_repo_policies_give_every_scenarios_latency() {
    let s = cards::scenarios::parse(include_str!("../../../config/policies.toml")).unwrap();
    let got: Vec<(&str, i64)> = s.iter().map(|x| (x.name.as_str(), x.latency_ms)).collect();
    assert_eq!(
        got,
        vec![
            ("colo-idle", 52),
            ("sp-idle", 300),
            ("sp-burst", 450),
            ("sp-idle-nodelay", 300),
            ("rung-0", 0),
            ("rung-25", 25),
            ("rung-100", 100),
            ("rung-200", 200)
        ]
    );
    for name in cards::edge::LADDER.iter().chain(cards::edge::EXTRAS.iter()) {
        assert!(
            s.iter().any(|x| x.name == *name),
            "{name} is in policies.toml"
        );
    }
}

#[test]
fn a_file_that_is_not_toml_is_an_error() {
    assert!(cards::scenarios::parse("[[scenarios]\nname = ").is_err());
}
