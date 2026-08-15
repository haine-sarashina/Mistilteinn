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

/// Maximum nesting depth for @import resolution to prevent infinite recursion.
const MAX_IMPORT_DEPTH: usize = 10;

/// Maximum total number of @import requests across all levels.
const MAX_TOTAL_IMPORTS: usize = 50;

/// Result of a network fetch, including the resolved (post-redirect) URL.
pub struct FetchResult {
    pub content: String,
    /// The final URL after following all redirects. If no redirect occurred,
    /// this equals the original request URL.
    pub final_url: String,
}

/// Fetches the content of a URL with a proper User-Agent and timeout.
/// If an HTTPS request times out or fails to connect, automatically falls back to HTTP
/// to resolve domains that redirect via port 80 (e.g. apple.co.jp -> https://www.apple.com/jp/).
pub async fn fetch(url: &str) -> Result<FetchResult, NetworkError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(NetworkError::Http)?;

    match client.get(url).send().await {
        Ok(response) => {
            let final_url = response.url().to_string();
            let content = response.text().await?;
            Ok(FetchResult { content, final_url })
        }
        Err(e) => {
            if url.starts_with("https://") {
                let http_url = format!("http://{}", &url[8..]);
                log::info!(
                    "HTTPS fetch failed ({:?}), attempting HTTP fallback: {}",
                    e,
                    http_url
                );
                if let Ok(response) = client.get(&http_url).send().await {
                    let final_url = response.url().to_string();
                    let content = response.text().await?;
                    return Ok(FetchResult { content, final_url });
                }
            }
            Err(NetworkError::Http(e))
        }
    }
}

/// Resolve a potentially relative URL against a base URL.
/// - If the href is already absolute (starts with http:// or https://), return as-is.
/// - If it starts with "//", inherit the scheme from the base URL (protocol-relative).
/// - If it starts with "/", prepend the scheme + host from the base URL.
/// - Otherwise, append to the base URL's directory.
pub fn resolve_url(base_url: &str, href: &str) -> String {
    if href.starts_with("data:") || href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }

    // Protocol-relative URL: inherit scheme from base
    if href.starts_with("//") {
        let scheme = if base_url.starts_with("https://") {
            "https:"
        } else if base_url.starts_with("http://") {
            "http:"
        } else {
            return href.to_string();
        };
        return format!("{}{}", scheme, href);
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

use std::sync::LazyLock;

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mistilteinn/0.2.10 (https://github.com/haine-sarashina/Mistilteinn; dev@mistilteinn.local) reqwest/0.12")
        .build()
        .expect("Failed to build global HTTP client")
});

/// Simple base64 decoder for data: URIs without external dependency.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;

    for &b in input.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Fetch image bytes from a URL (or decode data: URI) with Keep-Alive connection pooling and retries.
pub async fn fetch_image(url: &str) -> Result<Vec<u8>, NetworkError> {
    if url.starts_with("data:") {
        if let Some(comma_pos) = url.find(',') {
            let meta = &url[..comma_pos];
            let data_str = &url[comma_pos + 1..];
            if meta.contains(";base64") {
                if let Some(decoded) = decode_base64(data_str.trim()) {
                    return Ok(decoded);
                }
            } else {
                // Raw / utf8 data: URI (e.g. data:image/svg+xml;utf8,<svg>...)
                let unescaped = data_str
                    .replace("%20", " ")
                    .replace("%3C", "<")
                    .replace("%3E", ">")
                    .replace("%23", "#")
                    .replace("%22", "\"");
                return Ok(unescaped.into_bytes());
            }
        }
    }

    for attempt in 0..3 {
        let resp = HTTP_CLIENT.get(url).send().await;
        match resp {
            Ok(res) if res.status().is_success() => {
                let bytes = res.bytes().await.map_err(NetworkError::Http)?;
                return Ok(bytes.to_vec());
            }
            Ok(res) if res.status().as_u16() == 429 => {
                tokio::time::sleep(std::time::Duration::from_millis(150 * (attempt + 1))).await;
            }
            Ok(_) => {
                break;
            }
            Err(e) => {
                if attempt == 2 {
                    return Err(NetworkError::Http(e));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    Err(NetworkError::Timeout)
}

/// Fetch a single CSS file with a shorter timeout (10s) and the standard User-Agent.
async fn fetch_css_file(url: &str) -> Result<String, NetworkError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(NetworkError::Http)?;

    let response = client.get(url).send().await?.text().await?;
    Ok(response)
}

/// Recursively resolve @import rules in CSS text.
///
/// Parses the CSS to find any `@import` directives, fetches each referenced URL,
/// and recursively resolves imports within those fetched files. Imported CSS is
/// merged BEFORE the original CSS text so the original has higher source-order priority
/// per the CSS specification.
///
/// - `css_text`: the CSS source to process
/// - `base_url`: the base URL for resolving relative import URLs
/// - `visited`: set of already-visited URLs (prevents circular imports)
/// - `depth`: current recursion depth (0 = top-level)
pub async fn resolve_imports(
    css_text: &str,
    base_url: &str,
    visited: &mut std::collections::HashSet<String>,
    depth: usize,
) -> String {
    // Check depth limit
    if depth > MAX_IMPORT_DEPTH {
        log::warn!(
            "Max @import depth ({}) reached — stopping recursion",
            MAX_IMPORT_DEPTH
        );
        return css_text.to_string();
    }

    // Check total import count limit
    if visited.len() > MAX_TOTAL_IMPORTS {
        log::warn!(
            "Max total @import count ({}) reached — stopping resolution",
            MAX_TOTAL_IMPORTS
        );
        return css_text.to_string();
    }

    // Parse the CSS to find @import rules
    let stylesheet = crate::css::parser::parse_stylesheet(css_text);

    if stylesheet.imports.is_empty() {
        // No imports — return original text unchanged
        return css_text.to_string();
    }

    log::info!(
        "Found {} @import rule(s) at depth {}, resolving...",
        stylesheet.imports.len(),
        depth
    );

    let mut imported_css_parts = Vec::new();

    for import_rule in &stylesheet.imports {
        let resolved_url = resolve_url(base_url, &import_rule.url);

        // Skip already-visited URLs (prevents circular imports)
        if visited.contains(&resolved_url) {
            log::info!("Skipping already-visited @import URL: {}", resolved_url);
            continue;
        }

        visited.insert(resolved_url.clone());

        match fetch_css_file(&resolved_url).await {
            Ok(content) => {
                log::info!(
                    "Fetched @import stylesheet: {} ({} bytes)",
                    resolved_url,
                    content.len()
                );
                // Recursively resolve imports in the fetched content via Box::pin
                // to avoid infinitely-sized future type from direct async recursion.
                let resolved =
                    Box::pin(resolve_imports(&content, &resolved_url, visited, depth + 1)).await;
                imported_css_parts.push(resolved);
            }
            Err(e) => {
                log::warn!(
                    "Failed to fetch @import stylesheet {}: {:?}",
                    resolved_url,
                    e
                );
            }
        }
    }

    // Merge: imported CSS comes BEFORE original CSS (original has higher source-order priority)
    let merged_imports = imported_css_parts.join("\n");
    if merged_imports.is_empty() {
        return css_text.to_string();
    }
    format!("{}\n{}", merged_imports, css_text)
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

    let merged_css = merged.join("\n");

    // Resolve @import rules in the merged CSS
    let mut visited = std::collections::HashSet::new();
    // Mark all already-fetched external URLs as visited so we don't re-fetch them
    for url in &resolved_urls {
        visited.insert(url.clone());
    }
    let final_css = resolve_imports(&merged_css, base_url, &mut visited, 0).await;

    Ok(final_css)
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
    fn test_resolve_protocol_relative_url_https() {
        // Protocol-relative URL should inherit https scheme from base
        assert_eq!(
            resolve_url("https://example.com/page", "//cdn.example.com/style.css"),
            "https://cdn.example.com/style.css"
        );
    }

    #[test]
    fn test_resolve_protocol_relative_url_http() {
        // Protocol-relative URL should inherit http scheme from base
        assert_eq!(
            resolve_url("http://example.com/page", "//cdn.example.com/style.css"),
            "http://cdn.example.com/style.css"
        );
    }

    #[test]
    fn test_extract_external_css_urls_max_limit() {
        // Generate HTML with 60 <link> tags — only MAX_EXTERNAL_SHEETS (50) should be fetched
        let mut html = String::from("<html><head>");
        for i in 0..60 {
            html.push_str(&format!(
                r#"<link rel="stylesheet" href="/css/style{}.css">"#,
                i
            ));
        }
        html.push_str("</head><body></body></html>");

        let urls = extract_external_css_urls(&html);
        // extract_external_css_urls returns all URLs; fetch_external_css caps at MAX_EXTERNAL_SHEETS
        assert_eq!(urls.len(), 60, "extract should find all 60 URLs");

        // Verify the cap constant
        assert_eq!(MAX_EXTERNAL_SHEETS, 50);
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

    #[test]
    fn test_resolve_image_relative_path() {
        assert_eq!(
            resolve_url("https://example.com/page/", "images/logo.png"),
            "https://example.com/page/images/logo.png"
        );
    }

    #[test]
    fn test_resolve_image_root_path() {
        assert_eq!(
            resolve_url("https://example.com/page/index.html", "/assets/img.png"),
            "https://example.com/assets/img.png"
        );
    }

    #[test]
    fn test_resolve_image_already_absolute() {
        assert_eq!(
            resolve_url("https://example.com", "https://cdn.example.com/image.jpg"),
            "https://cdn.example.com/image.jpg"
        );
    }
}
