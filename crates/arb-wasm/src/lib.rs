//! arb-wasm: the public playground's engine (Phase 7: 09-27 spec §4, 7b spec §3.4). All logic is
//! arb-core's and sim's; `api` is the JSON facade (plain Rust, tested natively), and the
//! wasm32-only wrappers below only forward strings.
#![forbid(unsafe_code)]

pub mod api;
pub mod input;

#[cfg(target_arch = "wasm32")]
mod bindings {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen]
    pub fn mirror_json(input: &str) -> String {
        crate::api::mirror_json(input)
    }

    #[wasm_bindgen]
    pub fn payoff_json(input: &str) -> String {
        crate::api::payoff_json(input)
    }
}
