//! What `GET /api/cards/meta` lists besides the tick (spec §8.1, §5.5). analyze writes these to
//! card_meta at startup, so the api stays a pass-through.
use serde_json::{Value, json};

pub const SAGUILLO_NOTE: &str =
    "coming later: needs the on-chain dataset and somewhere to put it that is not your laptop";

pub fn papers() -> Value {
    json!([
        {
            "id": "gebele-2026",
            "cite": "Gebele, Mutzel, Matthes (2026). Executable Arbitrage and Market Efficiency in Prediction Markets. arXiv 2608.00666 v1.",
            "status": "live",
            "note": null,
            "cards": ["G1", "G2", "G3", "G4"]
        },
        { "id": "saguillo", "cite": "Saguillo et al.", "status": "coming_later", "note": SAGUILLO_NOTE, "cards": [] },
        { "id": "laya", "cite": "Laya et al.", "status": "coming_later", "note": "Phase 6.", "cards": [] },
        { "id": "cheng", "cite": "Cheng et al.", "status": "dropped", "note": "dropped: the owner's call, not a data problem", "cards": [] }
    ])
}

pub fn not_replicated() -> Value {
    json!([
        {
            "item": "Converter profits",
            "why": "They come from on-chain conversions, which we don't observe."
        },
        {
            "item": "Same-condition findings of the WebSocket validation appendix (20 inconsistencies in 308,416,666 messages)",
            "why": "They need the on-chain side too. The Health page reports our own integrity rate instead."
        }
    ])
}
