// `sqlx::migrate!` embeds crates/store/migrations at compile time, but on stable Rust it cannot
// see a NEW file in that directory, so rebuild whenever it changes (sqlx docs: "sqlx migrate
// build-script").
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
