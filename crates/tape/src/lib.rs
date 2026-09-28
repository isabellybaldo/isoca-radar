//! tape: the raw capture as both the scanner and the compactor read it (spec 5b §2, "reuse, not a
//! second implementation"): capture and control lines, Polymarket frame parsing, the capture file
//! layout, and the scanner engine's book rules (`feed`, Tasks 2–3).
#![forbid(unsafe_code)]

pub mod feed;
pub mod files;
pub mod frame;
pub mod line;
pub mod time;
pub mod universe_view;
