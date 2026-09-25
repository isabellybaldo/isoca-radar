//! Postgres TEXT cannot store NUL; replace it with U+FFFD so external strings never poison a write.
pub(crate) fn clean(s: &str) -> String {
    if s.contains('\0') {
        s.replace('\0', "\u{FFFD}")
    } else {
        s.to_string()
    }
}

pub(crate) fn clean_opt(s: &Option<String>) -> Option<String> {
    s.as_deref().map(clean)
}
