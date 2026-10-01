//! Payoff tables (parent spec §5.7 (b); Phase 7 spec §3.1). Rows are holdings, columns are the
//! winner. `broken[c]`: a possible column where the basket pays less than it guarantees (L3).
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PayoffError {
    #[error("a basket needs 2 to 12 outcomes, got {0}")]
    Size(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    pub possible: Vec<bool>,
    pub cells: Vec<Vec<Option<Decimal>>>,
    pub basket: Vec<Option<Decimal>>,
    pub guaranteed: Decimal,
    pub broken: Vec<bool>,
}

const NAMES: [&str; 12] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L"];

fn check(s: usize) -> Result<(), PayoffError> {
    if (2..=12).contains(&s) {
        Ok(())
    } else {
        Err(PayoffError::Size(s))
    }
}

fn cols(s: usize, other_listed: bool) -> Vec<String> {
    let mut c: Vec<String> = NAMES.iter().take(s).map(|n| (*n).to_string()).collect();
    if !other_listed {
        c.push("Other".to_string());
    }
    c
}

fn finish(
    rows: Vec<String>,
    cols: Vec<String>,
    possible: Vec<bool>,
    cells: Vec<Vec<Option<Decimal>>>,
    guaranteed: Decimal,
) -> Matrix {
    let basket: Vec<Option<Decimal>> = (0..cols.len())
        .map(|c| {
            if possible[c] {
                Some(cells.iter().filter_map(|r| r[c]).sum())
            } else {
                None
            }
        })
        .collect();
    let broken = (0..cols.len())
        .map(|c| possible[c] && basket[c].is_some_and(|b| b < guaranteed))
        .collect();
    Matrix {
        rows,
        cols,
        possible,
        cells,
        basket,
        guaranteed,
        broken,
    }
}

fn unit(pays: bool) -> Option<Decimal> {
    Some(if pays { Decimal::ONE } else { Decimal::ZERO })
}

/// D2a: one YES of each listed outcome. Pays 1 if a listed outcome wins, 0 if "Other" does (L3).
pub fn yes_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError> {
    check(s)?;
    let cols = cols(s, other_listed);
    let rows = NAMES.iter().take(s).map(|n| format!("YES_{n}")).collect();
    let cells = (0..s)
        .map(|i| (0..cols.len()).map(|c| unit(c == i)).collect())
        .collect();
    let possible = vec![true; cols.len()];
    Ok(finish(rows, cols, possible, cells, Decimal::ONE))
}

/// D2b: one NO of each listed outcome. Pays s − 1 if a listed outcome wins, s otherwise (L4).
pub fn no_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError> {
    check(s)?;
    let cols = cols(s, other_listed);
    let rows = NAMES.iter().take(s).map(|n| format!("NO_{n}")).collect();
    let cells = (0..s)
        .map(|i| (0..cols.len()).map(|c| unit(c != i)).collect())
        .collect();
    let possible = vec![true; cols.len()];
    let guaranteed = Decimal::from(s) - Decimal::ONE;
    Ok(finish(rows, cols, possible, cells, guaranteed))
}

/// D3: A ⇒ B, holding NO_A + YES_B. A∧¬B is impossible (L10).
pub fn implication() -> Matrix {
    let cols: Vec<String> = ["A∧B", "A∧¬B", "¬A∧B", "¬A∧¬B"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    let possible = vec![true, false, true, true];
    let (z, o) = (unit(false), unit(true));
    let cells = vec![vec![z, None, o, o], vec![o, None, o, z]];
    finish(
        vec!["NO_A".into(), "YES_B".into()],
        cols,
        possible,
        cells,
        Decimal::ONE,
    )
}
