#![allow(clippy::unwrap_used, clippy::expect_used)]
use pm_api::clob::ClobClient;
use pm_api::gamma::{GammaClient, token_ids};

#[tokio::test]
#[ignore]
async fn live_gamma_page_and_books() {
    pm_api::init_tls();
    let http = reqwest::Client::new();
    let page = GammaClient::new(http.clone()).page(None).await.unwrap();
    assert!(!page.events.is_empty());
    let tokens: Vec<String> = page
        .events
        .iter()
        .flat_map(|e| e.markets().iter())
        .filter_map(|m| token_ids(m).ok().flatten().map(|(y, _)| y))
        .take(5)
        .collect();
    let books = ClobClient::new(http).books(&tokens).await.unwrap();
    for b in &books {
        b.to_book().unwrap();
    }
}
