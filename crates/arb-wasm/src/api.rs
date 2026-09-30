//! JSON facade over arb-core and sim (09-27 spec §4.1, 7b spec §3.4): each `<name>_json` parses,
//! checks the limits, calls arb-core/sim and prints. It computes no money figure; its one decision
//! is which update is in force at t_x (`episode_json`, 09-27 §4.1).
use crate::input::{MAX_INPUT, d};
use arb_core::money::{Price, Shares};
use serde::Serialize;
use serde::de::DeserializeOwned;

mod classify;
mod depth_walk;
mod episode;
mod mirror;
mod payoff;

pub use classify::classify_json;
pub use depth_walk::depth_walk_json;
pub use episode::episode_json;
pub use mirror::mirror_json;
pub use payoff::payoff_json;

#[derive(Serialize)]
struct ErrOut<'a> {
    error: &'a str,
}

fn err(m: &str) -> String {
    serde_json::to_string(&ErrOut { error: m })
        .unwrap_or_else(|_| r#"{"error":"internal"}"#.to_string())
}

/// Size check, parse, run, print. Every failure is one `{"error": "<sentence>"}`.
fn run<I: DeserializeOwned, O: Serialize>(
    input: &str,
    f: impl FnOnce(I) -> Result<O, String>,
) -> String {
    if input.len() > MAX_INPUT {
        return err("input is larger than 16 KiB");
    }
    let parsed: I = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => {
            return err(&format!(
                "not valid input: {}",
                e.to_string().replace('\n', " ")
            ));
        }
    };
    match f(parsed).and_then(|o| serde_json::to_string(&o).map_err(|e| e.to_string())) {
        Ok(s) => s,
        Err(m) => err(&m),
    }
}

#[derive(Serialize)]
struct LevelOut {
    p: String,
    s: String,
}

fn lv((p, s): (Price, Shares)) -> LevelOut {
    LevelOut {
        p: d(p.get()),
        s: d(s.get()),
    }
}
