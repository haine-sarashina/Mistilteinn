pub mod cache;
pub mod security;
pub mod websocket;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Timeout")]
    Timeout,
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    /// The server's certificate could not be verified.
    ///
    /// Separate from [`NetworkError::Http`] because it is the one failure the
    /// user can knowingly override, and the one that must never be retried
    /// over plaintext.
    #[error("certificate error for {host}: {detail}")]
    Certificate { host: String, detail: String },
    /// A security policy refused the request before it was made.
    #[error("blocked: {0}")]
    Blocked(String),
    /// The server answered, but with nothing to render.
    ///
    /// Kept separate from [`NetworkError::Http`] because it is not a transport
    /// failure: without it an empty body silently becomes a blank page with no
    /// indication that anything went wrong.
    #[error("{0}")]
    EmptyResponse(String),
}

/// Max number of external stylesheets to fetch per page.
const MAX_EXTERNAL_SHEETS: usize = 50;

/// Maximum nesting depth for @import resolution to prevent infinite recursion.
const MAX_IMPORT_DEPTH: usize = 10;

/// Maximum total number of @import requests across all levels.
const MAX_TOTAL_IMPORTS: usize = 50;

/// How much of the body the meta-charset prescan looks at, per the HTML spec.
const CHARSET_PRESCAN_BYTES: usize = 1024;

/// The scheme of pages the browser itself produces, as opposed to pages it
/// fetched. They are a separate origin from any site, and some of them can act
/// on the user's behalf, so a site must never be able to navigate to one.
pub const INTERNAL_SCHEME: &str = "mistilteinn://";

/// Result of a network fetch, including the resolved (post-redirect) URL.
pub struct FetchResult {
    pub content: String,
    /// The final URL after following all redirects. If no redirect occurred,
    /// this equals the original request URL.
    pub final_url: String,
    /// The encoding the body was decoded with, for logging and diagnostics.
    pub encoding: &'static str,
    /// The `Content-Security-Policy` headers the document was served with.
    /// A document may send several, and all of them apply.
    pub csp: Vec<String>,
}

/// Decode a response body the way a browser does.
///
/// The order is the HTML spec's: a byte order mark wins outright, then the
/// `Content-Type` charset, then a `<meta>` declaration found by scanning the
/// first kilobyte of the body, and finally UTF-8.
///
/// The prescan is why this is not simply `Response::text()`: reqwest only
/// consults the `Content-Type` header, so a Shift_JIS page that declares its
/// encoding in `<meta>` alone — as most older Japanese sites do — decoded as
/// mojibake.
pub fn decode_body(bytes: &[u8], content_type: Option<&str>) -> (String, &'static str) {
    if let Some((encoding, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = encoding.decode_without_bom_handling(&bytes[bom_len..]);
        return (text.into_owned(), encoding.name());
    }

    let encoding = content_type
        .and_then(charset_from_content_type)
        .or_else(|| prescan_meta_charset(bytes))
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);

    let (text, _, _) = encoding.decode(bytes);
    (text.into_owned(), encoding.name())
}

/// Pull the `charset` parameter out of a `Content-Type` header value.
fn charset_from_content_type(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let idx = lower.find("charset")?;
    let rest = value[idx + "charset".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    let end = rest
        .find(|c: char| c == ';' || c.is_whitespace())
        .unwrap_or(rest.len());
    let label = rest[..end].trim_matches(|c| c == '"' || c == '\'').trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// Scan the start of a document for a `<meta>` charset declaration.
///
/// Deliberately a byte scan rather than a parse: the encoding has to be known
/// *before* the document can be decoded, so there is nothing to parse yet.
/// Both spellings are recognised — `<meta charset=…>` and the older
/// `<meta http-equiv="content-type" content="…; charset=…">`.
fn prescan_meta_charset(bytes: &[u8]) -> Option<String> {
    let window = &bytes[..bytes.len().min(CHARSET_PRESCAN_BYTES)];
    // The prescan runs over ASCII markup, so a lossy conversion is safe here:
    // any non-ASCII byte can only be inside content we are not looking at.
    let text = String::from_utf8_lossy(window).to_ascii_lowercase();

    let mut search_from = 0usize;
    while let Some(offset) = text[search_from..].find("<meta") {
        let tag_start = search_from + offset;
        let tag_end = text[tag_start..]
            .find('>')
            .map(|e| tag_start + e)
            .unwrap_or(text.len());
        let attrs = tag_attributes(&text[tag_start..tag_end]);
        search_from = tag_end.max(tag_start + 5);

        let lookup = |name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };

        // `<meta charset="shift_jis">`
        if let Some(label) = lookup("charset") {
            return Some(label.to_string());
        }
        // `<meta http-equiv="content-type" content="text/html; charset=euc-jp">`
        if let Some(label) = lookup("content").and_then(charset_from_content_type) {
            return Some(label);
        }
    }
    None
}

/// Split a raw tag into its attribute name/value pairs.
///
/// Attributes are parsed rather than searched for, because `charset` also
/// occurs *inside* the value of `content="text/html; charset=euc-jp"` — a
/// substring search would read the wrong one and stop at the wrong delimiter.
pub(crate) fn tag_attributes(tag: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let bytes: Vec<char> = tag.chars().collect();
    let mut i = 0usize;

    // Skip `<` and the tag name.
    while i < bytes.len() && !bytes[i].is_whitespace() {
        i += 1;
    }

    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_whitespace() || bytes[i] == '/') {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_whitespace() && bytes[i] != '=' && bytes[i] != '/' {
            i += 1;
        }
        if i == name_start {
            break;
        }
        let name: String = bytes[name_start..i].iter().collect();

        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < bytes.len() && bytes[i] == '=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == '"' || bytes[i] == '\'') {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    value.push(bytes[i]);
                    i += 1;
                }
                i += 1; // closing quote
            } else {
                while i < bytes.len() && !bytes[i].is_whitespace() && bytes[i] != '>' {
                    value.push(bytes[i]);
                    i += 1;
                }
            }
        }
        attrs.push((name, value.trim().to_string()));
    }

    attrs
}

/// Fetches the content of a URL with a proper User-Agent and timeout.
/// If an HTTPS request times out or fails to connect, automatically falls back to HTTP
/// to resolve domains that redirect via port 80 (e.g. apple.co.jp -> https://www.apple.com/jp/).
pub async fn fetch(url: &str) -> Result<FetchResult, NetworkError> {
    let client = client_for(url);
    let request = || client.get(url).timeout(std::time::Duration::from_secs(12));

    match request().send().await {
        Ok(response) => into_fetch_result(response).await,
        Err(e) => {
            // A certificate failure is reported, never worked around. Retrying
            // over HTTP would answer "this connection cannot be trusted" by
            // dropping the encryption altogether, and would do it silently.
            if security::is_certificate_error(&e) {
                let host = security::host_of(url).unwrap_or_else(|| url.to_string());
                log::warn!("certificate error for {host}: {e}");
                return Err(NetworkError::Certificate {
                    host,
                    detail: e.to_string(),
                });
            }

            if url.starts_with("https://") {
                let http_url = format!("http://{}", &url[8..]);
                log::info!(
                    "HTTPS fetch failed ({:?}), attempting HTTP fallback: {}",
                    e,
                    http_url
                );
                if let Ok(response) = client
                    .get(&http_url)
                    .timeout(std::time::Duration::from_secs(12))
                    .send()
                    .await
                {
                    return into_fetch_result(response).await;
                }
            }
            Err(NetworkError::Http(e))
        }
    }
}

/// Describe an empty response, naming bot protection when the server admits to it.
fn empty_response_detail(final_url: &str, status: u16, waf_action: Option<&str>) -> String {
    match waf_action {
        Some(action) => format!(
            "{final_url} returned HTTP {status} with an empty body \
             (bot protection: x-amzn-waf-action: {action}). \
             Passing it needs JavaScript execution and cookie storage."
        ),
        None => format!("{final_url} returned HTTP {status} with an empty body"),
    }
}

/// Turn a response into a [`FetchResult`], rejecting ones with nothing to show.
///
/// A non-2xx status is *not* by itself an error: servers routinely send a
/// readable 404 or 500 page, and a browser is expected to render it. What
/// cannot be rendered is an empty body, which previously sailed through as a
/// successful load and painted a blank window with no explanation.
async fn into_fetch_result(response: reqwest::Response) -> Result<FetchResult, NetworkError> {
    let final_url = response.url().to_string();
    let status = response.status();

    // AWS WAF signals "solve a JS challenge and retry" with this header and an
    // empty body. Naming it is far more useful than a bare "empty response".
    let waf_challenge = response
        .headers()
        .get("x-amzn-waf-action")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase());

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    let csp: Vec<String> = response
        .headers()
        .get_all("content-security-policy")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|v| v.to_string())
        .collect();

    // Take the raw bytes rather than `Response::text()`: the encoding may only
    // be declared in a `<meta>` tag, which reqwest does not look at.
    let bytes = response.bytes().await?;
    let (content, encoding) = decode_body(&bytes, content_type.as_deref());

    if content.trim().is_empty() {
        let detail = empty_response_detail(&final_url, status.as_u16(), waf_challenge.as_deref());
        log::warn!("{detail}");
        return Err(NetworkError::EmptyResponse(detail));
    }

    if !status.is_success() {
        log::warn!("{final_url} returned HTTP {status}; rendering the body anyway");
    }
    if encoding != "UTF-8" {
        log::info!("{final_url} decoded as {encoding}");
    }

    Ok(FetchResult {
        content,
        final_url,
        encoding,
        csp,
    })
}

/// Resolve a potentially relative URL against a base URL.
/// - If the href is already absolute (starts with http:// or https://), return as-is.
/// - If it starts with "//", inherit the scheme from the base URL (protocol-relative).
/// - If it starts with "/", prepend the scheme + host from the base URL.
/// - Otherwise, append to the base URL's directory.
pub fn resolve_url(base_url: &str, href: &str) -> String {
    if href.starts_with("data:")
        || href.starts_with("http://")
        || href.starts_with("https://")
        // An internal page is an absolute reference too. Resolving it as a
        // relative path would hand the browser's own pages to whatever site
        // happened to be open.
        || href.starts_with(INTERNAL_SCHEME)
    {
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

use std::sync::{Arc, LazyLock};

/// What we tell servers we are.
///
/// A browser UA rather than our own name: a great many sites serve a different
/// document, or refuse outright, to anything they do not recognise.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// How many redirects to follow before giving up.
const MAX_REDIRECTS: usize = 10;

/// The cookie jar shared by every request.
///
/// Cookies belong to an origin, not to a tab, so one jar for the process is
/// what a browser does. It lives only as long as the process — nothing is
/// written to disk, so every run starts a fresh session.
static COOKIE_JAR: LazyLock<Arc<reqwest::cookie::Jar>> =
    LazyLock::new(|| Arc::new(reqwest::cookie::Jar::default()));

/// The jar backing [`http_client`], for inspection and for seeding cookies.
pub fn cookie_jar() -> &'static Arc<reqwest::cookie::Jar> {
    &COOKIE_JAR
}

/// The one HTTP client every request goes through.
///
/// Sharing it is what makes cookies work at all: a client built per request
/// starts with an empty jar, so a `Set-Cookie` was accepted and then
/// immediately thrown away. It also means connections, DNS results and the
/// redirect limit are shared rather than rebuilt for every image on a page.
///
/// No timeout is set here — each caller applies its own with
/// `RequestBuilder::timeout`, since a stylesheet and a page do not deserve the
/// same patience.
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .cookie_provider(COOKIE_JAR.clone())
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .build()
        .expect("Failed to build global HTTP client")
});

/// The shared client. See [`HTTP_CLIENT`].
pub fn http_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}

/// The client used for hosts the user has granted a certificate exception to.
///
/// It shares the cookie jar with [`HTTP_CLIENT`] — it is the same browsing
/// session — and differs only in accepting a certificate that could not be
/// verified. It is reached solely through [`client_for`], which requires an
/// exception the user granted on the warning page for that exact host.
static INSECURE_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .cookie_provider(COOKIE_JAR.clone())
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("Failed to build the certificate-exception HTTP client")
});

/// The client to send this request with: the verifying one, unless the user has
/// explicitly accepted this host's certificate.
fn client_for(url: &str) -> &'static reqwest::Client {
    match security::host_of(url) {
        Some(host) if security::has_cert_exception(&host) => {
            log::warn!("{host}: proceeding with an unverified certificate at the user's request");
            &INSECURE_CLIENT
        }
        _ => &HTTP_CLIENT,
    }
}

/// Fetch a subresource that the CORS rules apply to, such as a web font.
///
/// Sends the document's `Origin` and refuses a cross-origin response the server
/// did not agree to share. A font is the case that matters here: browsers fetch
/// them in CORS mode precisely so a site cannot quietly read a font file it was
/// not given permission to.
pub async fn fetch_cors(
    url: &str,
    document_url: &str,
    timeout: std::time::Duration,
) -> Result<Vec<u8>, NetworkError> {
    if url.starts_with("data:") {
        return fetch_image(url).await;
    }

    let mut request = client_for(url).get(url).timeout(timeout);
    if let Some(origin) = security::Origin::parse(document_url) {
        request = request.header(reqwest::header::ORIGIN, origin.serialize());
    }

    let response = request.send().await?;
    let allow_origin = response
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let final_url = response.url().to_string();

    if !security::cors_allows(document_url, &final_url, allow_origin.as_deref()) {
        return Err(NetworkError::Blocked(format!(
            "{final_url} was refused by CORS (Access-Control-Allow-Origin: {})",
            allow_origin.as_deref().unwrap_or("absent")
        )));
    }

    Ok(response.bytes().await?.to_vec())
}

/// A body obtained either from the network or from the HTTP cache.
pub struct CachedFetch {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    /// The URL the body came from, after redirects.
    pub final_url: String,
    /// The response status, or `None` when nothing was requested because the
    /// cached copy was still fresh.
    pub status: Option<reqwest::StatusCode>,
}

/// GET a URL through the HTTP cache.
///
/// A fresh entry is returned without touching the network. A stale one that
/// carries a validator is revalidated, which normally costs a 304 and no body
/// at all — that is where the saving is on a revisit, since most static assets
/// ship an `ETag` but no lifetime.
pub async fn get_with_cache(
    url: &str,
    timeout: std::time::Duration,
) -> Result<CachedFetch, NetworkError> {
    let cached = cache::lookup(url);
    let mut validators = None;

    if let Some(entry) = &cached {
        match cache::decide(&entry.policy, entry.age()) {
            cache::CacheDecision::Fresh => {
                log::debug!("cache hit: {url}");
                return Ok(CachedFetch {
                    bytes: entry.body.clone(),
                    content_type: entry.content_type.clone(),
                    final_url: entry.final_url.clone(),
                    status: None,
                });
            }
            cache::CacheDecision::Revalidate {
                if_none_match,
                if_modified_since,
            } => validators = Some((if_none_match, if_modified_since)),
            cache::CacheDecision::Miss => {}
        }
    }

    let mut request = http_client().get(url).timeout(timeout);
    if let Some((if_none_match, if_modified_since)) = &validators {
        if let Some(etag) = if_none_match {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(since) = if_modified_since {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, since);
        }
    }

    let response = request.send().await?;
    let status = response.status();

    if status == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(entry) = cached {
            log::debug!("cache revalidated: {url}");
            cache::refresh(url);
            return Ok(CachedFetch {
                bytes: entry.body,
                content_type: entry.content_type,
                final_url: entry.final_url,
                status: Some(status),
            });
        }
    }

    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let policy = cache::CachePolicy::from_headers(response.headers());
    let bytes = response.bytes().await?.to_vec();

    if status.is_success() {
        cache::store(
            url,
            cache::new_entry(
                bytes.clone(),
                content_type.clone(),
                final_url.clone(),
                policy,
            ),
        );
    }

    Ok(CachedFetch {
        bytes,
        content_type,
        final_url,
        status: Some(status),
    })
}

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
        let resp = get_with_cache(url, std::time::Duration::from_secs(15)).await;
        match resp {
            // `None` means the cached copy was still fresh; a 304 means it was
            // revalidated. Either way the bytes are good.
            Ok(res) if res.status.is_none_or(|s| s.is_success() || s == 304) => {
                return Ok(res.bytes);
            }
            Ok(res) if res.status.is_some_and(|s| s.as_u16() == 429) => {
                tokio::time::sleep(std::time::Duration::from_millis(150 * (attempt + 1))).await;
            }
            Ok(_) => {
                break;
            }
            Err(e) => {
                if attempt == 2 {
                    return Err(e);
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    Err(NetworkError::Timeout)
}

/// Fetch a single CSS file with a shorter timeout (10s) and the standard User-Agent.
async fn fetch_css_file(url: &str) -> Result<String, NetworkError> {
    let fetched = get_with_cache(url, std::time::Duration::from_secs(10)).await?;
    // Stylesheets are far more often UTF-8 than documents are, but a BOM or a
    // declared charset still has to be honoured.
    let (content, _) = decode_body(&fetched.bytes, fetched.content_type.as_deref());
    Ok(content)
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
    csp: &security::Csp,
) -> Result<String, NetworkError> {
    // An inline `<style>` block is inline content in the CSP sense, so a
    // policy without `'unsafe-inline'` drops it exactly as it drops an inline
    // script.
    let inline_css = if csp.allows_inline(security::ResourceKind::Style) {
        extract_css(html_content)
    } else {
        log::warn!("inline <style> blocked by this page's Content-Security-Policy");
        String::new()
    };
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

    // Resolve all relative URLs against the base URL, then put each through
    // the page's own policy and the mixed-content rules. A stylesheet can
    // rewrite the whole page, so an insecure one is dropped rather than
    // upgraded and hoped for.
    let resolved_urls: Vec<String> = limited_hrefs
        .iter()
        .map(|href| resolve_url(base_url, href))
        .filter_map(|url| {
            match security::check_subresource(base_url, csp, &url, security::ResourceKind::Style) {
                security::SubresourceDecision::Load(url) => Some(url),
                security::SubresourceDecision::Block(reason) => {
                    log::warn!("stylesheet blocked: {reason}");
                    None
                }
            }
        })
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

    #[test]
    fn empty_response_detail_names_bot_protection() {
        let detail = empty_response_detail("https://www.amazon.co.jp/", 202, Some("challenge"));
        assert!(detail.contains("https://www.amazon.co.jp/"));
        assert!(detail.contains("202"));
        assert!(
            detail.contains("bot protection"),
            "a WAF challenge must be named, not reported as a generic failure: {detail}"
        );
        assert!(
            detail.contains("JavaScript"),
            "says what it would take: {detail}"
        );
    }

    #[test]
    fn empty_response_detail_without_waf_header() {
        let detail = empty_response_detail("https://example.com/", 204, None);
        assert!(detail.contains("204"));
        assert!(detail.contains("empty body"));
        assert!(
            !detail.contains("bot protection"),
            "do not claim bot protection without the header: {detail}"
        );
    }

    #[tokio::test]
    #[ignore] // Requires network access and Amazon may return WAF challenge page
    async fn test_fetch_amazon() {
        // amazon.co.jp answers automated clients with an AWS WAF challenge:
        // HTTP 202, `x-amzn-waf-action: challenge`, and a zero-length body.
        // The old version of this test only checked the final URL, so it passed
        // green while the fetch returned nothing and the page rendered blank.
        //
        // Either outcome is legitimate depending on what the WAF decides, but a
        // silent empty success is not: assert we either get real content or a
        // named EmptyResponse.
        match fetch("https://www.amazon.co.jp").await {
            Ok(r) => {
                assert!(
                    r.final_url.contains("amazon"),
                    "should stay on amazon: {}",
                    r.final_url
                );
                assert!(
                    !r.content.trim().is_empty(),
                    "a successful fetch must carry a body"
                );
            }
            Err(NetworkError::EmptyResponse(detail)) => {
                println!("amazon.co.jp is gating this client: {detail}");
                assert!(detail.contains("amazon"), "detail names the URL: {detail}");
            }
            Err(e) => panic!("unexpected transport failure: {e:?}"),
        }
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

    /// Encode `text` with a named encoding, for round-trip decode tests.
    fn encode_as(label: &str, text: &str) -> Vec<u8> {
        let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).unwrap();
        encoding.encode(text).0.into_owned()
    }

    /// The cookies the jar would send with a request to `url`.
    fn cookies_for(jar: &reqwest::cookie::Jar, url: &str) -> String {
        use reqwest::cookie::CookieStore;
        jar.cookies(&url.parse().unwrap())
            .map(|v| v.to_str().unwrap_or_default().to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_cookie_is_sent_back_to_the_origin_that_set_it() {
        // Using a fresh jar rather than the shared one so the test does not
        // depend on what else has run.
        let jar = reqwest::cookie::Jar::default();
        let url = "https://example.com/".parse().unwrap();
        jar.add_cookie_str("session=abc123; Path=/", &url);

        assert_eq!(
            cookies_for(&jar, "https://example.com/account"),
            "session=abc123"
        );
    }

    #[test]
    fn a_cookie_is_not_leaked_to_another_origin() {
        let jar = reqwest::cookie::Jar::default();
        jar.add_cookie_str(
            "session=secret; Path=/",
            &"https://example.com/".parse().unwrap(),
        );

        assert_eq!(
            cookies_for(&jar, "https://attacker.test/"),
            "",
            "a cookie must not travel to a different host"
        );
    }

    #[test]
    fn cookie_paths_are_respected() {
        let jar = reqwest::cookie::Jar::default();
        jar.add_cookie_str(
            "scoped=1; Path=/admin",
            &"https://example.com/".parse().unwrap(),
        );

        assert_eq!(
            cookies_for(&jar, "https://example.com/admin/users"),
            "scoped=1"
        );
        assert_eq!(
            cookies_for(&jar, "https://example.com/public"),
            "",
            "a path-scoped cookie stays inside its path"
        );
    }

    #[test]
    fn the_shared_client_carries_the_shared_jar() {
        // The wiring is what matters: a client built per request would accept a
        // Set-Cookie and then discard it with the client.
        let jar = cookie_jar();
        jar.add_cookie_str(
            "wired=yes; Path=/",
            &"https://mistilteinn.test/".parse().unwrap(),
        );
        assert_eq!(cookies_for(jar, "https://mistilteinn.test/"), "wired=yes");

        // And the client is genuinely one instance, not rebuilt per call.
        assert!(std::ptr::eq(http_client(), http_client()));
    }

    #[test]
    fn meta_charset_decodes_a_shift_jis_page() {
        // The header says nothing; only the document declares the encoding.
        let html = "<html><head><meta charset=\"Shift_JIS\"></head><body>日本語</body></html>";
        let bytes = encode_as("shift_jis", html);
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "the fixture must really be Shift_JIS, not UTF-8"
        );

        let (decoded, encoding) = decode_body(&bytes, Some("text/html"));
        assert_eq!(encoding, "Shift_JIS");
        assert!(decoded.contains("日本語"), "got: {decoded}");
    }

    #[test]
    fn http_equiv_meta_is_understood_too() {
        let html = "<html><head><meta http-equiv=\"Content-Type\" \
                    content=\"text/html; charset=EUC-JP\"></head><body>日本語</body></html>";
        let bytes = encode_as("euc-jp", html);

        let (decoded, encoding) = decode_body(&bytes, None);
        assert_eq!(encoding, "EUC-JP");
        assert!(decoded.contains("日本語"), "got: {decoded}");
    }

    #[test]
    fn content_type_header_beats_the_document() {
        // A header charset is authoritative; the meta tag must not override it.
        let html = "<html><head><meta charset=\"utf-8\"></head><body>日本語</body></html>";
        let bytes = encode_as("shift_jis", html);

        let (decoded, encoding) = decode_body(&bytes, Some("text/html; charset=Shift_JIS"));
        assert_eq!(encoding, "Shift_JIS");
        assert!(decoded.contains("日本語"), "got: {decoded}");
    }

    #[test]
    fn a_bom_wins_over_every_declaration() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("<meta charset=\"shift_jis\">日本語".as_bytes());

        let (decoded, encoding) = decode_body(&bytes, Some("text/html; charset=euc-jp"));
        assert_eq!(encoding, "UTF-8");
        assert!(decoded.contains("日本語"), "got: {decoded}");
        assert!(!decoded.starts_with('\u{feff}'), "the BOM is consumed");
    }

    #[test]
    fn plain_utf8_is_unaffected() {
        let html = "<html><body>日本語 and ASCII</body></html>";
        let (decoded, encoding) = decode_body(html.as_bytes(), Some("text/html; charset=utf-8"));
        assert_eq!(encoding, "UTF-8");
        assert_eq!(decoded, html);
    }

    #[test]
    fn a_meta_charset_past_the_prescan_window_is_not_used() {
        // Browsers only scan the first kilobyte; a declaration after it is too
        // late to change the decode.
        let padding = " ".repeat(CHARSET_PRESCAN_BYTES + 64);
        let html = format!("<html><head>{padding}<meta charset=\"shift_jis\"></head></html>");
        assert_eq!(decode_body(html.as_bytes(), None).1, "UTF-8");
    }

    #[test]
    fn charset_from_content_type_handles_quotes_and_extra_params() {
        assert_eq!(
            charset_from_content_type("text/html; charset=\"Shift_JIS\"; boundary=x").as_deref(),
            Some("Shift_JIS")
        );
        assert_eq!(
            charset_from_content_type("text/html; Charset = euc-jp").as_deref(),
            Some("euc-jp")
        );
        assert_eq!(charset_from_content_type("text/html").as_deref(), None);
    }

    #[test]
    fn prescan_ignores_attributes_that_merely_end_in_charset() {
        let html = "<html><head><meta data-charset=\"shift_jis\"></head></html>";
        assert_eq!(prescan_meta_charset(html.as_bytes()), None);
    }

    #[test]
    fn an_unknown_charset_label_falls_back_to_utf8() {
        let html = "<html><head><meta charset=\"not-a-real-encoding\"></head><body>x</body></html>";
        assert_eq!(decode_body(html.as_bytes(), None).1, "UTF-8");
    }

    #[test]
    fn test_fetch_result_fields() {
        let fr = FetchResult {
            content: "hello".to_string(),
            final_url: "https://example.com".to_string(),
            encoding: "UTF-8",
            csp: Vec::new(),
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

        let result =
            fetch_external_css("https://example.com", html, &security::Csp::default()).await;
        assert!(result.is_ok());
        let css = result.unwrap();
        assert!(css.contains("margin: 0"));
    }

    #[tokio::test]
    async fn test_fetch_external_css_empty_graceful() {
        let html = r#"<!DOCTYPE html><html><head></head><body></body></html>"#;

        let result =
            fetch_external_css("https://example.com", html, &security::Csp::default()).await;
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
