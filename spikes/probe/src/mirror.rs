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

/// "mirror" | "MISMATCH" | "parse_error" | "missing", by comparing the YES/NO books for one pair.
fn status_of(books: &HashMap<String, BookWire>, yes: &str, no: &str) -> &'static str {
    match (books.get(yes), books.get(no)) {
        (Some(y), Some(n)) => match (y.to_book(), n.to_book()) {
            (Ok(yb), Ok(nb)) if mirror_matches(&yb, &nb) => "mirror",
            (Ok(_), Ok(_)) => "MISMATCH",
            _ => "parse_error",
        },
        _ => "missing",
    }
}

/// The first `limit` NegRisk legs, walking groups in order and pairing `yes_tokens[i]`/
/// `no_tokens[i]` within each group (a leg with no matching `no_tokens` entry is skipped).
fn first_legs(u: &Universe, limit: usize) -> Vec<(String, String, String)> {
    let mut legs = Vec::new();
    for g in &u.groups {
        for i in 0..g.yes_tokens.len().min(g.no_tokens.len()) {
            if legs.len() >= limit {
                return legs;
            }
            legs.push((
                format!("{}#{i}", g.event_id),
                g.yes_tokens[i].clone(),
                g.no_tokens[i].clone(),
            ));
        }
    }
    legs
}

pub async fn mirror(u: &Universe, out: &Path) -> anyhow::Result<()> {
    let clob = ClobClient::new()?;
    let legs = first_legs(u, 400);
    let mut tokens: Vec<String> = u
        .canary
        .iter()
        .flat_map(|c| [c.yes.clone(), c.no.clone()])
        .collect();
    tokens.extend(legs.iter().flat_map(|(_, y, n)| [y.clone(), n.clone()]));
    let books: HashMap<String, BookWire> = clob
        .books(&tokens)
        .await?
        .into_iter()
        .map(|b| (b.asset_id.clone(), b))
        .collect();

    let mut lines = vec!["id,kind,status,yes_levels".to_string()];
    let (mut c_ok, mut c_bad, mut c_missing) = (0usize, 0usize, 0usize);
    for c in &u.canary {
        let status = status_of(&books, &c.yes, &c.no);
        match status {
            "mirror" => c_ok += 1,
            "missing" => c_missing += 1,
            _ => c_bad += 1,
        }
        let levels = books.get(&c.yes).map_or(0, |b| b.bids.len() + b.asks.len());
        lines.push(format!("{},canary,{status},{levels}", c.market_id));
    }
    let (mut n_ok, mut n_bad, mut n_missing) = (0usize, 0usize, 0usize);
    for (id, yes, no) in &legs {
        let status = status_of(&books, yes, no);
        match status {
            "mirror" => n_ok += 1,
            "missing" => n_missing += 1,
            _ => n_bad += 1,
        }
        let levels = books.get(yes).map_or(0, |b| b.bids.len() + b.asks.len());
        lines.push(format!("{id},negrisk,{status},{levels}"));
    }
    lines.push(format!(
        "# canary_mirror={c_ok} canary_mismatch_or_error={c_bad} canary_missing={c_missing} negrisk_mirror={n_ok} negrisk_mismatch_or_error={n_bad} negrisk_missing={n_missing}"
    ));
    std::fs::write(out, lines.join("\n") + "\n")?;
    println!(
        "canary: mirror={c_ok} mismatch_or_error={c_bad} missing={c_missing}; negrisk: mirror={n_ok} mismatch_or_error={n_bad} missing={n_missing}"
    );
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

    #[test]
    fn first_legs_walks_groups_in_order_and_stops_at_the_limit() {
        use crate::universe::Group;
        let groups = vec![
            Group {
                event_id: "1".into(),
                title: "A".into(),
                yes_tokens: vec!["a1y".into(), "a2y".into()],
                no_tokens: vec!["a1n".into(), "a2n".into()],
            },
            Group {
                event_id: "2".into(),
                title: "B".into(),
                yes_tokens: vec!["b1y".into()],
                no_tokens: vec!["b1n".into()],
            },
        ];
        let u = Universe {
            groups,
            canary: Vec::new(),
        };
        let legs = first_legs(&u, 2);
        assert_eq!(
            legs,
            vec![
                ("1#0".to_string(), "a1y".to_string(), "a1n".to_string()),
                ("1#1".to_string(), "a2y".to_string(), "a2n".to_string()),
            ],
            "limit of 2 is hit inside the first group; group 2 is never reached"
        );
        assert_eq!(first_legs(&u, 100).len(), 3, "no limit hit → all 3 legs");
    }
}
