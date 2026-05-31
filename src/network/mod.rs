use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Timeout")]
    Timeout,
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// Fetches the content of a URL.
pub async fn fetch(url: &str) -> Result<String, NetworkError> {
    let response = reqwest::get(url).await?.text().await?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_fetch_amazon() {
        let content = fetch("https://www.amazon.co.jp").await;
        assert!(content.is_ok());
        assert!(content.unwrap().contains("html"));
    }
}
