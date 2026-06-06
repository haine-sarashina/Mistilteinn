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

/// Extract CSS from HTML content — inline <style> blocks and <link> references.
/// Returns the merged CSS string (inline styles only for now, external links noted).
pub fn extract_css(html_content: &str) -> String {
    let arena = crate::html::parser::parse_html(html_content);
    let mut css_parts = Vec::new();

    // Walk the DOM tree looking for <style> elements — collect their text content
    // Also note any <link rel="stylesheet" href="..."> elements (log but don't fetch externally yet)
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
            } else if tag.map(|t| t.as_ref() == "link").unwrap_or(false) {
                // Check for stylesheet links and log them
                if let Some(rel) = node.get_attr("rel") {
                    if rel.eq_ignore_ascii_case("stylesheet") {
                        if let Some(href) = node.get_attr("href") {
                            log::info!("External stylesheet found (not fetched yet): {}", href);
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
}
