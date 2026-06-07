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

/// Max number of external stylesheets to fetch per page.
const MAX_EXTERNAL_SHEETS: usize = 50;

/// Result of a network fetch, including the resolved (post-redirect) URL.
pub struct FetchResult {
    pub content: String,
    /// The final URL after following all redirects. If no redirect occurred,
    /// this equals the original request URL.
    pub final_url: String,
}

/// Fetches the content of a URL with a proper User-Agent and timeout.
pub async fn fetch(url: &str) -> Result<FetchResult, NetworkError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 Mistilteinn/0.1")
        .build()
        .map_err(NetworkError::Http)?;

    let response = client.get(url).send().await?;
    let final_url = response.url().to_string();
    let content = response.text().await?;
    Ok(FetchResult { content, final_url })
}

/// Resolve a potentially relative URL against a base URL.
/// - If the href is already absolute (starts with http://, https://, or //), return as-is.
/// - If it starts with "/", prepend the scheme + host from the base URL.
/// - Otherwise, append to the base URL's directory.
pub fn resolve_url(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//") {
        return href.to_string();
    }

    if let Some(slash_href) = href.strip_prefix('/') {
        // Absolute path: prepend scheme + host
        if let Some(rest) = base_url.strip_prefix("http://") {
            if let Some(host) = rest.split('/').next() {
                return format!("http://{}/{}", host, slash_href);
            }
        }
        if let Some(rest) = base_url.strip_prefix("https://") {
            if let Some(host) = rest.split('/').next() {
                return format!("https://{}/{}", host, slash_href);
            }
        }
    } else {
        // Relative path: append to base URL's directory
        let base_dir = if let Some(last_slash) = base_url.rfind('/') {
            base_url[..last_slash + 1].to_string()
        } else {
            base_url.to_string()
        };
        return format!("{}{}", base_dir, href);
    }

    // Fallback: return the href as-is
    href.to_string()
}

/// Extract external stylesheet URLs from HTML content.
/// Finds all `<link rel="stylesheet" href="...">` elements and returns their href values.
pub fn extract_external_css_urls(html_content: &str) -> Vec<String> {
    let arena = crate::html::parser::parse_html(html_content);
    let mut urls = Vec::new();

    fn walk_node(
        arena: &crate::html::DomArena,
        node_id: crate::html::NodeId,
        urls: &mut Vec<String>,
    ) {
        let node = match arena.get(crate::html::DomHandle(node_id)) {
            Some(n) => n,
            None => return,
        };

        if node.is_element() {
            let tag = node.tag_name();
            if tag.map(|t| t.as_ref() == "link").unwrap_or(false) {
                if let Some(rel) = node.get_attr("rel") {
                    if rel.eq_ignore_ascii_case("stylesheet") {
                        if let Some(href) = node.get_attr("href") {
                            urls.push(href.to_string());
                        }
                    }
                }
            }
        }

        for &child_id in node.children() {
            walk_node(arena, child_id, urls);
        }
    }

    walk_node(&arena, crate::html::NodeId::DOCUMENT, &mut urls);
    urls
}

/// Extract CSS from HTML content — inline `<style>` blocks only.
/// Returns the merged CSS string from all inline style elements.
pub fn extract_css(html_content: &str) -> String {
    let arena = crate::html::parser::parse_html(html_content);
    let mut css_parts = Vec::new();

    // Walk the DOM tree looking for <style> elements — collect their text content
    fn walk_node(
        arena: &crate::html::DomArena,
        node_id: crate::html::NodeId,
        css_parts: &mut Vec<String>,
    ) {
        let node = match arena.get(crate::html::DomHandle(node_id)) {
            Some(n) => n,
            None => return,
        };

        if node.is_element() {
            let tag = node.tag_name();
            if tag.map(|t| t.as_ref() == "style").unwrap_or(false) {
                // Collect text content from child text nodes
                for &child_id in node.children() {
                    if let Some(child) = arena.get(crate::html::DomHandle(child_id)) {
                        if let Some(text) = child.text_content() {
                            css_parts.push(text.to_string());
                        }
                    }
                }
            }
        }

        // Recurse into children
        for &child_id in node.children() {
            walk_node(arena, child_id, css_parts);
        }
    }

    // Start walking from the document root (node 0)
    walk_node(&arena, crate::html::NodeId::DOCUMENT, &mut css_parts);

    css_parts.join("\n")
}

/// Fetch a single CSS file with a shorter timeout (10s) and the standard User-Agent.
async fn fetch_css_file(url: &str) -> Result<String, NetworkError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 Mistilteinn/0.1")
        .build()
        .map_err(NetworkError::Http)?;

    let response = client.get(url).send().await?.text().await?;
    Ok(response)
}

/// Fetch all external stylesheets concurrently and merge their content with inline CSS.
/// Resolves relative URLs against `base_url` before fetching.
/// Caps the number of fetched stylesheets at `MAX_EXTERNAL_SHEETS`.
/// Returns the combined CSS string (inline styles + fetched external stylesheets).
pub async fn fetch_external_css(
    base_url: &str,
    html_content: &str,
) -> Result<String, NetworkError> {
    let inline_css = extract_css(html_content);
    let external_hrefs = extract_external_css_urls(html_content);

    if external_hrefs.is_empty() {
        return Ok(inline_css);
    }

    // Cap the number of external stylesheets to prevent excessive fetches
    let total_count = external_hrefs.len();
    let limited_hrefs: Vec<String> = external_hrefs
        .into_iter()
        .take(MAX_EXTERNAL_SHEETS)
        .collect();

    if limited_hrefs.len() < total_count {
        log::warn!(
            "Limited external stylesheets from {} to {}",
            total_count,
            MAX_EXTERNAL_SHEETS
        );
    }

    log::info!(
        "Found {} external stylesheet(s), fetching concurrently",
        limited_hrefs.len()
    );

    // Resolve all relative URLs against the base URL
    let resolved_urls: Vec<String> = limited_hrefs
        .iter()
        .map(|href| resolve_url(base_url, href))
        .collect();

    // Fetch all external stylesheets concurrently using join_all
    let fetch_futures = resolved_urls.iter().map(|url| {
        let url_clone = url.clone();
        async move {
            match fetch_css_file(&url_clone).await {
                Ok(content) => {
                    log::info!(
                        "Fetched external stylesheet: {} ({} bytes)",
                        url_clone,
                        content.len()
                    );
                    Some(content)
                }
                Err(e) => {
                    log::warn!("Failed to fetch external stylesheet {}: {:?}", url_clone, e);
                    None
                }
            }
        }
    });

    let results = futures::future::join_all(fetch_futures).await;

    // Merge inline CSS with all successfully fetched external CSS
    let mut merged = Vec::new();
    merged.push(inline_css);
    for result in results.into_iter().flatten() {
        merged.push(result);
    }

    Ok(merged.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network access and Amazon may return WAF challenge page
    async fn test_fetch_amazon() {
        let result = fetch("https://www.amazon.co.jp").await;
        assert!(result.is_ok(), "Fetch should succeed");
        let fetch_result = result.unwrap();
        assert!(
            !fetch_result.final_url.is_empty(),
            "final_url should be set"
        );
        assert!(
            fetch_result.final_url.contains("amazon"),
            "Should redirect to amazon domain: {}",
            fetch_result.final_url
        );
        // Note: Amazon may return empty content or a WAF challenge page for automated requests.
        // Content validation is skipped in CI/automated environments.
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_fetch_example_com() {
        let result = fetch("http://example.com").await;
        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert!(
            fetch_result.content.contains("<!DOCTYPE") || fetch_result.content.contains("<html")
        );
        assert!(fetch_result.final_url.starts_with("http://example.com"));
    }

    #[test]
    fn test_fetch_result_fields() {
        let fr = FetchResult {
            content: "hello".to_string(),
            final_url: "https://example.com".to_string(),
        };
        assert_eq!(fr.content, "hello");
        assert_eq!(fr.final_url, "https://example.com");
    }

    #[test]
    fn test_extract_css_from_style_tags() {
        let html = r#"<!DOCTYPE html><html><head>
            <style>body { margin: 0; }</style>
            <style>.foo { color: red; }</style>
        </head><body><div>Hello</div></body></html>"#;

        let css = extract_css(html);
        assert!(css.contains("margin: 0"));
        assert!(css.contains("color: red"));
    }

    #[test]
    fn test_extract_css_empty_html() {
        let html = r#"<!DOCTYPE html><html><head></head><body></body></html>"#;
        let css = extract_css(html);
        assert!(css.is_empty());
    }

    #[test]
    fn test_extract_external_css_urls_single() {
        let html = r#"<!DOCTYPE html><html><head>
            <link rel="stylesheet" href="/css/main.css">
        </head><body></body></html>"#;

        let urls = extract_external_css_urls(html);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "/css/main.css");
    }

    #[test]
    fn test_extract_external_css_urls_multiple() {
        let html = r#"<!DOCTYPE html><html><head>
            <link rel="stylesheet" href="https://cdn.example.com/lib.css">
            <link rel="stylesheet" href="/css/main.css">
            <link rel="stylesheet" href="styles/theme.css">
            <link rel="icon" href="/favicon.ico">
        </head><body></body></html>"#;

        let urls = extract_external_css_urls(html);
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://cdn.example.com/lib.css");
        assert_eq!(urls[1], "/css/main.css");
        assert_eq!(urls[2], "styles/theme.css");
    }

    #[test]
    fn test_extract_external_css_urls_ignores_non_stylesheet() {
        let html = r#"<!DOCTYPE html><html><head>
            <link rel="icon" href="/favicon.ico">
            <link rel="preconnect" href="https://fonts.googleapis.com">
        </head><body></body></html>"#;

        let urls = extract_external_css_urls(html);
        assert_eq!(urls.len(), 0);
    }

    #[test]
    fn test_resolve_absolute_https_url() {
        assert_eq!(
            resolve_url(
                "https://example.com/page",
                "https://cdn.example.com/style.css"
            ),
            "https://cdn.example.com/style.css"
        );
    }

    #[test]
    fn test_resolve_absolute_http_url() {
        assert_eq!(
            resolve_url("https://example.com/page", "http://other.com/style.css"),
            "http://other.com/style.css"
        );
    }

    #[test]
    fn test_resolve_protocol_relative_url() {
        assert_eq!(
            resolve_url("https://example.com/page", "//cdn.example.com/style.css"),
            "//cdn.example.com/style.css"
        );
    }

    #[test]
    fn test_resolve_root_relative_url() {
        assert_eq!(
            resolve_url("https://example.com/page/index.html", "/css/main.css"),
            "https://example.com/css/main.css"
        );
    }

    #[test]
    fn test_resolve_root_relative_url_http() {
        assert_eq!(
            resolve_url("http://example.com/path/", "/styles.css"),
            "http://example.com/styles.css"
        );
    }

    #[test]
    fn test_resolve_relative_url_with_directory() {
        assert_eq!(
            resolve_url("https://example.com/page/index.html", "css/style.css"),
            "https://example.com/page/css/style.css"
        );
    }

    #[test]
    fn test_resolve_relative_url_no_trailing_slash() {
        assert_eq!(
            resolve_url("https://example.com/page", "style.css"),
            "https://example.com/style.css"
        );
    }

    #[tokio::test]
    async fn test_fetch_external_css_no_external() {
        let html = r#"<!DOCTYPE html><html><head>
            <style>body { margin: 0; }</style>
        </head><body></body></html>"#;

        let result = fetch_external_css("https://example.com", html).await;
        assert!(result.is_ok());
        let css = result.unwrap();
        assert!(css.contains("margin: 0"));
    }

    #[tokio::test]
    async fn test_fetch_external_css_empty_graceful() {
        let html = r#"<!DOCTYPE html><html><head></head><body></body></html>"#;

        let result = fetch_external_css("https://example.com", html).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
