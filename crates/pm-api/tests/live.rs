#![allow(clippy::unwrap_used, clippy::expect_used)]
use pm_api::clob::ClobClient;
use pm_api::gamma::{GammaClient, token_ids};

#[tokio::test]
#[ignore]
async fn live_gamma_page_and_books() {
    let page = GammaClient::new().unwrap().page(None).await.unwrap();
    assert!(!page.events.is_empty());
    let tokens: Vec<String> = page
        .events
        .iter()
        .flat_map(|e| e.markets().iter())
        .filter_map(|m| token_ids(m).ok().flatten().map(|(y, _)| y))
        .take(5)
        .collect();
    let books = ClobClient::new().unwrap().books(&tokens).await.unwrap();
    for b in &books {
        b.to_book().unwrap();
    }
}

#[tokio::test]
#[ignore]
async fn live_gamma_pages_dont_overlap() {
    let client = GammaClient::new().unwrap();
    let page1 = client.page(None).await.unwrap();
    let cursor = page1
        .next_cursor
        .clone()
        .filter(|c| !c.is_empty())
        .expect("expected a second page to exist");
    let page2 = client.page(Some(cursor)).await.unwrap();
    let ids1: std::collections::HashSet<&str> =
        page1.events.iter().map(|e| e.id.as_str()).collect();
    for e in &page2.events {
        assert!(
            !ids1.contains(e.id.as_str()),
            "page 2 id {} also appeared on page 1",
            e.id
        );
    }
}
