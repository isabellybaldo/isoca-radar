//! Spec §16 #1: refuse to start if any exchange or wallet credential is present in the environment.
//! Database role passwords and DATABASE_URL are expected and allowed. Only NAMES are reported.
//! Same denylist as bins/scanner/src/guard.rs and bins/api/src/guard.rs (bins/scanner/tests/arch.rs keeps the three in step).

const CONTAINS: [&str; 5] = [
    "PRIVATE_KEY",
    "MNEMONIC",
    "SEED_PHRASE",
    "API_SECRET",
    "PASSPHRASE",
];
const PREFIXES: [&str; 2] = ["POLY_", "CLOB_"];

pub fn forbidden<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut bad: Vec<String> = names
        .into_iter()
        .map(|n| n.as_ref().to_string())
        .filter(|n| {
            let u = n.to_ascii_uppercase();
            CONTAINS.iter().any(|c| u.contains(c)) || PREFIXES.iter().any(|p| u.starts_with(p))
        })
        .collect();
    bad.sort();
    bad
}

pub fn check_env() -> anyhow::Result<()> {
    let bad = forbidden(std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()));
    anyhow::ensure!(
        bad.is_empty(),
        "refusing to start: exchange/wallet credential variables are set: {} (paper-only project, spec §16)",
        bad.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_wallet_and_exchange_names_but_not_database_settings() {
        let names = [
            "POLY_API_KEY",
            "WALLET_PRIVATE_KEY",
            "KEY_PASSPHRASE",
            "DATABASE_URL",
            "API_BIND",
            "API_MAX_CONNS",
            "PATH",
        ];
        assert_eq!(
            forbidden(names),
            vec!["KEY_PASSPHRASE", "POLY_API_KEY", "WALLET_PRIVATE_KEY"]
        );
    }
}
