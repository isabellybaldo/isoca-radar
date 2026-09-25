//! Spec §16 #1: refuse to start if any exchange or wallet credential is present in the environment.
//! Database role passwords (POSTGRES_*, PG*, *_DB_PASSWORD) are expected and allowed. Only NAMES are
//! ever reported, never values.

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
    fn flags_wallet_and_exchange_names_but_not_database_passwords() {
        let names = [
            "POLY_API_KEY",
            "CLOB_SECRET",
            "WALLET_PRIVATE_KEY",
            "my_mnemonic",
            "SEED_PHRASE_BACKUP",
            "EXCHANGE_API_SECRET",
            "KEY_PASSPHRASE",
            "POSTGRES_PASSWORD",
            "PGPASSWORD",
            "SCANNER_DB_PASSWORD",
            "DATABASE_URL",
            "PATH",
        ];
        assert_eq!(
            forbidden(names),
            vec![
                "CLOB_SECRET",
                "EXCHANGE_API_SECRET",
                "KEY_PASSPHRASE",
                "POLY_API_KEY",
                "SEED_PHRASE_BACKUP",
                "WALLET_PRIVATE_KEY",
                "my_mnemonic",
            ]
        );
    }
}
