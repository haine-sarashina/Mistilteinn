//! Fetch the benchmark page once and keep it, so the measurements that follow
//! are made against the same bytes every time.
#[tokio::main]
async fn main() {
    let url =
        "https://ja.wikipedia.org/wiki/%E3%83%A1%E3%82%A4%E3%83%B3%E3%83%9A%E3%83%BC%E3%82%B8";
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Mistilteinn/0.2")
        .build()
        .unwrap();
    let html = client.get(url).send().await.unwrap().text().await.unwrap();
    let css = mistilteinn::network::fetch_external_css(
        url,
        &html,
        &mistilteinn::network::security::Csp::default(),
    )
    .await
    .unwrap_or_else(|_| mistilteinn::network::extract_css(&html));
    let dir = std::env::args().nth(1).expect("output dir");
    std::fs::write(format!("{dir}/wiki.html"), &html).unwrap();
    std::fs::write(format!("{dir}/wiki.css"), &css).unwrap();
    println!("html {} bytes, css {} bytes", html.len(), css.len());
}
