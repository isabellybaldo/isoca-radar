//! L1 at scale: are NO books the exact mirror of YES books? (spec L1, Gebele App. A)
use crate::universe::Universe;
use arb_core::book::Book;
use pm_api::clob::{BookWire, ClobClient};
use std::collections::HashMap;
use std::path::Path;

pub fn mirror_matches(yes: &Book, no: &Book) -> bool {
    yes.no_asks_best_first() == no.asks_best_first()
        && yes.no_bids_best_first() == no.bids_best_first()
}

pub async fn mirror(u: &Universe, out: &Path) -> anyhow::Result<()> {
    let clob = ClobClient::new()?;
    let tokens: Vec<String> = u
        .canary
        .iter()
        .flat_map(|c| [c.yes.clone(), c.no.clone()])
        .collect();
    let books: HashMap<String, BookWire> = clob
        .books(&tokens)
        .await?
        .into_iter()
        .map(|b| (b.asset_id.clone(), b))
        .collect();
    let mut lines = vec!["market_id,status,yes_levels".to_string()];
    let (mut ok, mut bad, mut missing) = (0usize, 0usize, 0usize);
    for c in &u.canary {
        let status = match (books.get(&c.yes), books.get(&c.no)) {
            (Some(y), Some(n)) => match (y.to_book(), n.to_book()) {
                (Ok(yb), Ok(nb)) if mirror_matches(&yb, &nb) => {
                    ok += 1;
                    "mirror"
                }
                (Ok(_), Ok(_)) => {
                    bad += 1;
                    "MISMATCH"
                }
                _ => {
                    bad += 1;
                    "parse_error"
                }
            },
            _ => {
                missing += 1;
                "missing"
            }
        };
        let levels = books.get(&c.yes).map_or(0, |b| b.bids.len() + b.asks.len());
        lines.push(format!("{},{status},{levels}", c.market_id));
    }
    lines.push(format!(
        "# mirror={ok} mismatch_or_error={bad} missing={missing}"
    ));
    std::fs::write(out, lines.join("\n") + "\n")?;
    println!("mirror={ok} mismatch_or_error={bad} missing={missing}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arb_core::money::{Price, Shares};

    fn lv(p: &str, s: &str) -> (Price, Shares) {
        (Price::parse(p).unwrap(), Shares::parse(s).unwrap())
    }

    #[test]
    fn detects_mirror_and_its_absence() {
        let mut yes = Book::default();
        yes.apply_snapshot(
            &[lv("0.54", "5603.74"), lv("0.53", "85887.16")],
            &[lv("0.55", "32356.03")],
            1,
        );
        let mut no = Book::default();
        no.apply_snapshot(
            &[lv("0.45", "32356.03")],
            &[lv("0.46", "5603.74"), lv("0.47", "85887.16")],
            1,
        );
        assert!(mirror_matches(&yes, &no));
        no.apply_snapshot(&[lv("0.45", "32356.03")], &[lv("0.46", "5603.74")], 1); // one level missing
        assert!(!mirror_matches(&yes, &no));
    }
}
