//! Web security policy: origins, same-origin, mixed content, CORS and CSP.
//!
//! Every rule here answers the same question from a different angle — may this
//! document reach that URL? — so they share one notion of an origin and one
//! decision type. Deciding is kept separate from fetching: each rule is a pure
//! function over a URL and a policy, which is what makes them testable without
//! a network.

use std::collections::HashSet;

/// A web origin: scheme, host and port, as the same-origin rule defines it.
///
/// Two URLs are same-origin when all three match. The path never takes part —
/// `https://example.com/a` and `https://example.com/b` are the same origin, and
/// `https://example.com` and `http://example.com` are not.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

/// The port a scheme uses when the URL does not name one.
fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

impl Origin {
    /// The origin of a URL, or `None` for one that has no origin — a `data:`
    /// URI, an internal page, or anything unparseable.
    pub fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        let default = default_port(&scheme)?;

        // Authority runs to the first `/`, `?` or `#`.
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .filter(|a| !a.is_empty())?;
        // Credentials in the URL are not part of its origin.
        let authority = authority.rsplit('@').next()?;

        let (host, port) = match authority.rfind(':') {
            // A colon inside brackets belongs to an IPv6 literal, not a port.
            Some(index) if !authority[index..].contains(']') => {
                let port = authority[index + 1..].parse().ok()?;
                (&authority[..index], port)
            }
            _ => (authority, default),
        };

        if host.is_empty() {
            return None;
        }
        Some(Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        })
    }

    /// The origin in the form a browser sends in an `Origin` header.
    ///
    /// The port is written only when it is not the scheme's default, which is
    /// what a server comparing against its own allow-list expects to see.
    pub fn serialize(&self) -> String {
        if default_port(&self.scheme) == Some(self.port) {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }

    /// Whether content from this origin is delivered securely.
    ///
    /// Loopback counts: it cannot be tampered with in transit, which is the
    /// property the mixed-content rule is protecting.
    pub fn is_potentially_trustworthy(&self) -> bool {
        self.scheme == "https"
            || self.host == "localhost"
            || self.host.ends_with(".localhost")
            || self.host == "127.0.0.1"
            || self.host == "[::1]"
    }
}

/// Whether two URLs are same-origin.
///
/// A URL with no origin (a `data:` URI, an internal page) is same-origin with
/// nothing, including itself — it is opaque.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (Origin::parse(a), Origin::parse(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// What a document is loading, which decides both which CSP directive applies
/// and how seriously mixed content is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Document,
    Style,
    Script,
    Image,
    Font,
}

impl ResourceKind {
    /// The CSP directive that governs this kind of load.
    fn csp_directive(self) -> &'static str {
        match self {
            ResourceKind::Document => "default-src",
            ResourceKind::Style => "style-src",
            ResourceKind::Script => "script-src",
            ResourceKind::Image => "img-src",
            ResourceKind::Font => "font-src",
        }
    }

    /// Whether this kind can act on the page rather than only be shown in it.
    ///
    /// A tampered stylesheet, script or font can rewrite the page or read what
    /// is typed into it; a tampered image can only be the wrong picture. That
    /// is why browsers block the first group outright and merely upgrade the
    /// second.
    fn is_active(self) -> bool {
        !matches!(self, ResourceKind::Image)
    }
}

/// What to do with a subresource that a secure page loads insecurely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixedContent {
    /// Nothing insecure about it.
    Allowed,
    /// Passive content: try it over HTTPS instead of refusing outright.
    Upgrade(String),
    /// Active content on a secure page — refused.
    Blocked,
}

/// Decide what to do about the security of a subresource load.
///
/// A page served over HTTPS that pulls part of itself over HTTP is only as
/// secure as that plaintext part, so a browser refuses the dangerous half and
/// quietly upgrades the rest.
pub fn check_mixed_content(page_url: &str, resource_url: &str, kind: ResourceKind) -> MixedContent {
    let Some(page) = Origin::parse(page_url) else {
        // No origin to protect: an internal page or a local document.
        return MixedContent::Allowed;
    };
    if !page.is_potentially_trustworthy() {
        return MixedContent::Allowed;
    }
    let Some(resource) = Origin::parse(resource_url) else {
        // `data:` URIs and the like carry no network traffic to tamper with.
        return MixedContent::Allowed;
    };
    if resource.is_potentially_trustworthy() {
        return MixedContent::Allowed;
    }

    if kind.is_active() {
        MixedContent::Blocked
    } else {
        MixedContent::Upgrade(upgrade_to_https(resource_url))
    }
}

/// Rewrite an `http://` URL to `https://`, leaving anything else alone.
pub fn upgrade_to_https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

/// Whether a cross-origin response may be used, per its CORS headers.
///
/// `allow_origin` is the response's `Access-Control-Allow-Origin`. A same-origin
/// response never needs one; a cross-origin response needs `*` or this exact
/// origin, and anything else — including no header at all — means the server
/// did not agree to share it.
///
/// Simplification: requests are treated as anonymous, so `*` is accepted. A
/// browser rejects `*` for a credentialed request, but every CORS-mode fetch
/// here is for a font, which browsers fetch anonymously too.
pub fn cors_allows(page_url: &str, resource_url: &str, allow_origin: Option<&str>) -> bool {
    if same_origin(page_url, resource_url) {
        return true;
    }
    let Some(page) = Origin::parse(page_url) else {
        return false;
    };
    match allow_origin.map(str::trim) {
        Some("*") => true,
        Some(value) => Origin::parse(value).is_some_and(|allowed| allowed == page),
        None => false,
    }
}

/// What a document should do about one subresource it wants to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubresourceDecision {
    /// Fetch it, from this URL — which may not be the one the page asked for,
    /// if an insecure passive load was upgraded.
    Load(String),
    /// Do not fetch it, for this reason.
    Block(String),
}

/// Apply every rule that governs loading `url` as `kind`.
///
/// The page's own policy is consulted first: a CSP refusal is the site's own
/// decision and no upgrade can rescue it. Mixed content is second, and can
/// still rewrite the URL rather than refuse it.
pub fn check_subresource(
    document_url: &str,
    csp: &Csp,
    url: &str,
    kind: ResourceKind,
) -> SubresourceDecision {
    if !csp.allows(document_url, url, kind) {
        return SubresourceDecision::Block(format!(
            "{url} is not allowed by this page's Content-Security-Policy ({})",
            kind.csp_directive()
        ));
    }

    match check_mixed_content(document_url, url, kind) {
        MixedContent::Allowed => SubresourceDecision::Load(url.to_string()),
        MixedContent::Upgrade(secure) => SubresourceDecision::Load(secure),
        MixedContent::Blocked => SubresourceDecision::Block(format!(
            "{url} is insecure content on a secure page and was blocked"
        )),
    }
}

// ------ Content Security Policy ------

/// A parsed `Content-Security-Policy`.
///
/// Only the fetch directives are modelled, since those are the ones this engine
/// can act on. A policy with no directive covering a kind of load falls back to
/// `default-src`; with neither, the load is allowed.
#[derive(Debug, Clone, Default)]
pub struct Csp {
    directives: Vec<(String, Vec<Source>)>,
}

/// One entry in a directive's source list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// `'none'` — nothing matches, whatever else is in the list.
    None,
    /// `'self'` — the document's own origin.
    SelfOrigin,
    /// `'unsafe-inline'` — inline `<script>` and `<style>` may run.
    UnsafeInline,
    /// A scheme with no host, such as `https:` or `data:`.
    Scheme(String),
    /// A host, optionally with a scheme, a port, and a leading `*.`.
    Host {
        scheme: Option<String>,
        host: String,
        port: Option<u16>,
        /// `*.example.com` matches any subdomain, but not the bare domain.
        wildcard_subdomain: bool,
    },
    /// Anything at all.
    Any,
    /// A nonce, hash, or keyword this engine does not implement. Kept so a
    /// list is never mistaken for empty, but matches nothing.
    Unsupported,
}

impl Csp {
    /// Parse the policies a page declares.
    ///
    /// A page may send several — in more than one header, or in a `<meta>` tag
    /// as well — and they *all* apply: each one can only narrow what the others
    /// allow, never widen it. So they are merged into one set of directives and
    /// a load must satisfy every directive that governs it.
    pub fn parse(policies: &[String]) -> Self {
        let mut directives: Vec<(String, Vec<Source>)> = Vec::new();

        for policy in policies {
            for directive in policy.split(';') {
                let mut parts = directive.split_whitespace();
                let Some(name) = parts.next() else { continue };
                let name = name.to_ascii_lowercase();
                if !is_fetch_directive(&name) {
                    continue;
                }
                let sources: Vec<Source> = parts.map(parse_source).collect();
                directives.push((name, sources));
            }
        }

        Self { directives }
    }

    /// Whether any policy was declared at all.
    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }

    /// Whether `url` may be loaded as `kind` by a document at `document_url`.
    pub fn allows(&self, document_url: &str, url: &str, kind: ResourceKind) -> bool {
        self.each_governing(kind.csp_directive(), |sources| {
            sources
                .iter()
                .any(|source| source.matches(document_url, url))
        })
    }

    /// Whether an inline `<script>` or `<style>` may run.
    ///
    /// Blocking inline script is the single most common thing a CSP is set up
    /// to do, and this engine runs inline scripts, so it is a rule with real
    /// effect here rather than a formality.
    pub fn allows_inline(&self, kind: ResourceKind) -> bool {
        self.each_governing(kind.csp_directive(), |sources| {
            sources.contains(&Source::UnsafeInline)
        })
    }

    /// Apply `check` to every directive that governs `name`, requiring all of
    /// them to pass. `default-src` stands in only for a directive that is
    /// absent, which is what makes a narrow `img-src` override a wide default.
    fn each_governing(&self, name: &str, check: impl Fn(&[Source]) -> bool) -> bool {
        let mut governed = false;
        let mut allowed = true;

        for (directive, sources) in &self.directives {
            if directive == name {
                governed = true;
                allowed &= check(sources);
            }
        }
        if governed {
            return allowed;
        }

        for (directive, sources) in &self.directives {
            if directive == "default-src" {
                governed = true;
                allowed &= check(sources);
            }
        }
        // Nothing said anything about this kind of load, so nothing forbids it.
        !governed || allowed
    }
}

fn is_fetch_directive(name: &str) -> bool {
    matches!(
        name,
        "default-src"
            | "script-src"
            | "style-src"
            | "img-src"
            | "font-src"
            | "connect-src"
            | "frame-src"
            | "media-src"
    )
}

fn parse_source(token: &str) -> Source {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "'none'" => return Source::None,
        "'self'" => return Source::SelfOrigin,
        "'unsafe-inline'" => return Source::UnsafeInline,
        "*" => return Source::Any,
        _ => {}
    }
    if lower.starts_with('\'') {
        // A nonce, a hash, 'unsafe-eval', 'strict-dynamic': recognised as a
        // source we cannot evaluate rather than silently treated as a host.
        return Source::Unsupported;
    }

    // A bare scheme, `https:` or `data:`.
    if let Some(scheme) = lower.strip_suffix(':') {
        if !scheme.contains('/') && !scheme.is_empty() {
            return Source::Scheme(scheme.to_string());
        }
    }

    let (scheme, rest) = match lower.split_once("://") {
        Some((scheme, rest)) => (Some(scheme.to_string()), rest.to_string()),
        None => (None, lower),
    };
    // A path in a source expression restricts further; ignoring it only ever
    // allows more than the page asked for, so the host part is what we keep.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("").to_string();
    let (host_part, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() => (host.to_string(), port.parse().ok()),
        _ => (authority, None),
    };

    let (wildcard_subdomain, host) = match host_part.strip_prefix("*.") {
        Some(rest) => (true, rest.to_string()),
        None => (false, host_part),
    };

    if host.is_empty() {
        return Source::Unsupported;
    }
    Source::Host {
        scheme,
        host,
        port,
        wildcard_subdomain,
    }
}

impl Source {
    fn matches(&self, document_url: &str, url: &str) -> bool {
        match self {
            Source::None | Source::Unsupported => false,
            Source::UnsafeInline => false,
            Source::Any => true,
            Source::SelfOrigin => same_origin(document_url, url),
            Source::Scheme(scheme) => url
                .split_once(':')
                .is_some_and(|(s, _)| s.eq_ignore_ascii_case(scheme)),
            Source::Host {
                scheme,
                host,
                port,
                wildcard_subdomain,
            } => {
                let Some(origin) = Origin::parse(url) else {
                    return false;
                };
                if let Some(scheme) = scheme {
                    if &origin.scheme != scheme {
                        return false;
                    }
                }
                if let Some(port) = port {
                    if origin.port != *port {
                        return false;
                    }
                }
                if *wildcard_subdomain {
                    origin.host.ends_with(&format!(".{host}"))
                } else {
                    origin.host == *host
                }
            }
        }
    }
}

/// Pull `Content-Security-Policy` out of a document's `<meta http-equiv>` tags.
///
/// Read from the raw source rather than the DOM because the policy has to be
/// known before the document's own inline scripts are run, and running those is
/// part of building the DOM.
pub fn meta_csp(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut policies = Vec::new();
    let mut from = 0usize;

    while let Some(offset) = lower[from..].find("<meta") {
        let start = from + offset;
        let end = lower[start..]
            .find('>')
            .map(|e| start + e)
            .unwrap_or(lower.len());
        // Attribute values are read from the original text: the policy is
        // compared case-insensitively later, but a URL in it is not ours to
        // fold.
        let attrs = super::tag_attributes(&html[start..end]);
        let lookup = |name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        if lookup("http-equiv")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("content-security-policy"))
        {
            if let Some(content) = lookup("content") {
                policies.push(content.to_string());
            }
        }
        from = end.max(start + 5);
    }

    policies
}

// ------ Certificate exceptions ------

/// Hosts the user has chosen to visit despite a certificate they could not be
/// verified with.
///
/// Per host, never per session-wide flag, and only ever added by an explicit
/// click on the warning page: an exception is the user overruling a real
/// warning, so it must not be possible to acquire one by accident. Nothing is
/// written to disk, so exceptions last only as long as the process.
static CERT_EXCEPTIONS: std::sync::LazyLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashSet::new()));

/// Record that the user chose to proceed to `host` despite its certificate.
pub fn add_cert_exception(host: &str) {
    if let Ok(mut set) = CERT_EXCEPTIONS.lock() {
        set.insert(host.to_ascii_lowercase());
    }
}

/// Whether the user has already chosen to proceed to this host.
pub fn has_cert_exception(host: &str) -> bool {
    CERT_EXCEPTIONS
        .lock()
        .map(|set| set.contains(&host.to_ascii_lowercase()))
        .unwrap_or(false)
}

/// The host part of a URL, for looking up a certificate exception.
pub fn host_of(url: &str) -> Option<String> {
    Origin::parse(url).map(|o| o.host)
}

/// Whether a failed request failed because the server's certificate could not
/// be verified.
///
/// reqwest reports this as an ordinary connection error, so the cause chain has
/// to be read: the distinction matters because a certificate failure is the one
/// kind of connection failure that must never be retried over plaintext.
pub fn is_certificate_error(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        let text = current.to_string().to_ascii_lowercase();
        if text.contains("certificate")
            || text.contains("cert verify")
            || text.contains("unknownissuer")
            || text.contains("notvalidforname")
            || text.contains("certexpired")
            || text.contains("invalidcertificate")
            || text.contains("self-signed")
            || text.contains("self signed")
        {
            return true;
        }
        source = current.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_scheme_host_and_port() {
        let origin = Origin::parse("https://example.com/some/path?q=1").unwrap();
        assert_eq!(origin.scheme, "https");
        assert_eq!(origin.host, "example.com");
        assert_eq!(origin.port, 443, "the scheme's default port is implied");
        assert_eq!(origin.serialize(), "https://example.com");
    }

    #[test]
    fn a_declared_port_is_part_of_the_origin_and_of_its_serialization() {
        let origin = Origin::parse("http://example.com:8080/x").unwrap();
        assert_eq!(origin.port, 8080);
        assert_eq!(origin.serialize(), "http://example.com:8080");
    }

    #[test]
    fn the_path_does_not_take_part_in_the_origin() {
        assert!(same_origin(
            "https://example.com/a/b",
            "https://example.com/c"
        ));
    }

    #[test]
    fn scheme_host_and_port_must_all_match() {
        assert!(!same_origin("https://example.com/", "http://example.com/"));
        assert!(!same_origin(
            "https://example.com/",
            "https://other.example.com/"
        ));
        assert!(!same_origin(
            "https://example.com/",
            "https://example.com:8443/"
        ));
    }

    #[test]
    fn a_url_with_no_origin_is_same_origin_with_nothing() {
        // An opaque origin does not even match itself, which is what keeps a
        // `data:` document from reaching another one.
        assert!(!same_origin("data:text/html,x", "data:text/html,x"));
        assert!(!same_origin(
            "mistilteinn://bookmarks",
            "mistilteinn://bookmarks"
        ));
    }

    #[test]
    fn credentials_in_a_url_are_not_part_of_its_origin() {
        assert!(same_origin(
            "https://user:pass@example.com/",
            "https://example.com/"
        ));
    }

    #[test]
    fn a_secure_page_blocks_insecure_scripts_and_upgrades_insecure_images() {
        let page = "https://example.com/";
        assert_eq!(
            check_mixed_content(page, "http://cdn.example.com/app.js", ResourceKind::Script),
            MixedContent::Blocked
        );
        assert_eq!(
            check_mixed_content(page, "http://cdn.example.com/a.css", ResourceKind::Style),
            MixedContent::Blocked
        );
        assert_eq!(
            check_mixed_content(page, "http://cdn.example.com/f.woff", ResourceKind::Font),
            MixedContent::Blocked
        );
        assert_eq!(
            check_mixed_content(page, "http://cdn.example.com/a.png", ResourceKind::Image),
            MixedContent::Upgrade("https://cdn.example.com/a.png".to_string())
        );
    }

    #[test]
    fn an_insecure_page_is_left_alone() {
        // There is nothing to downgrade: the page itself arrived in plaintext.
        assert_eq!(
            check_mixed_content(
                "http://example.com/",
                "http://x.com/a.js",
                ResourceKind::Script
            ),
            MixedContent::Allowed
        );
    }

    #[test]
    fn https_subresources_of_a_secure_page_are_not_mixed_content() {
        assert_eq!(
            check_mixed_content(
                "https://example.com/",
                "https://cdn.example.com/a.js",
                ResourceKind::Script
            ),
            MixedContent::Allowed
        );
    }

    #[test]
    fn loopback_counts_as_secure() {
        // It cannot be tampered with in transit, which is the property the rule
        // protects — and blocking it would break local development.
        assert_eq!(
            check_mixed_content(
                "https://example.com/",
                "http://localhost:3000/a.js",
                ResourceKind::Script
            ),
            MixedContent::Allowed
        );
    }

    #[test]
    fn a_data_uri_is_not_mixed_content() {
        assert_eq!(
            check_mixed_content(
                "https://example.com/",
                "data:image/png;base64,AAAA",
                ResourceKind::Image
            ),
            MixedContent::Allowed
        );
    }

    #[test]
    fn a_same_origin_response_needs_no_cors_header() {
        assert!(cors_allows(
            "https://example.com/page",
            "https://example.com/font.woff",
            None
        ));
    }

    #[test]
    fn a_cross_origin_response_without_the_header_is_refused() {
        assert!(!cors_allows(
            "https://example.com/page",
            "https://cdn.other.com/font.woff",
            None
        ));
    }

    #[test]
    fn a_wildcard_or_a_matching_origin_allows_the_response() {
        let page = "https://example.com/page";
        let font = "https://cdn.other.com/font.woff";
        assert!(cors_allows(page, font, Some("*")));
        assert!(cors_allows(page, font, Some("https://example.com")));
        assert!(
            !cors_allows(page, font, Some("https://evil.com")),
            "a header naming someone else does not allow us"
        );
        assert!(
            !cors_allows(page, font, Some("http://example.com")),
            "the scheme has to match too"
        );
    }

    fn csp(policy: &str) -> Csp {
        Csp::parse(&[policy.to_string()])
    }

    #[test]
    fn a_policy_of_none_blocks_everything_of_that_kind() {
        let policy = csp("img-src 'none'");
        assert!(!policy.allows(
            "https://example.com/",
            "https://example.com/a.png",
            ResourceKind::Image
        ));
    }

    #[test]
    fn self_matches_the_documents_own_origin_only() {
        let policy = csp("img-src 'self'");
        let page = "https://example.com/page";
        assert!(policy.allows(page, "https://example.com/a.png", ResourceKind::Image));
        assert!(!policy.allows(page, "https://cdn.other.com/a.png", ResourceKind::Image));
    }

    #[test]
    fn default_src_stands_in_for_a_directive_that_is_absent() {
        let policy = csp("default-src 'self'");
        let page = "https://example.com/page";
        assert!(policy.allows(page, "https://example.com/a.png", ResourceKind::Image));
        assert!(!policy.allows(page, "https://cdn.other.com/a.png", ResourceKind::Image));
    }

    #[test]
    fn a_specific_directive_overrides_the_default_rather_than_adding_to_it() {
        let policy = csp("default-src 'none'; img-src *");
        let page = "https://example.com/page";
        assert!(
            policy.allows(page, "https://anywhere.com/a.png", ResourceKind::Image),
            "img-src replaces default-src for images"
        );
        assert!(
            !policy.allows(page, "https://anywhere.com/a.css", ResourceKind::Style),
            "and default-src still governs everything else"
        );
    }

    #[test]
    fn a_host_source_can_name_a_scheme_a_port_and_a_subdomain_wildcard() {
        let page = "https://example.com/page";

        let policy = csp("img-src https://cdn.example.com");
        assert!(policy.allows(page, "https://cdn.example.com/a.png", ResourceKind::Image));
        assert!(!policy.allows(page, "http://cdn.example.com/a.png", ResourceKind::Image));

        let policy = csp("img-src *.example.com");
        assert!(policy.allows(page, "https://cdn.example.com/a.png", ResourceKind::Image));
        assert!(
            !policy.allows(page, "https://example.com/a.png", ResourceKind::Image),
            "a subdomain wildcard does not cover the bare domain"
        );

        let policy = csp("img-src example.com:8080");
        assert!(policy.allows(page, "https://example.com:8080/a.png", ResourceKind::Image));
        assert!(!policy.allows(page, "https://example.com/a.png", ResourceKind::Image));
    }

    #[test]
    fn a_scheme_source_matches_any_host_on_that_scheme() {
        let policy = csp("img-src https:");
        let page = "https://example.com/page";
        assert!(policy.allows(page, "https://anywhere.com/a.png", ResourceKind::Image));
        assert!(!policy.allows(page, "http://anywhere.com/a.png", ResourceKind::Image));
    }

    #[test]
    fn inline_script_runs_only_when_the_policy_says_unsafe_inline() {
        assert!(!csp("script-src 'self'").allows_inline(ResourceKind::Script));
        assert!(csp("script-src 'self' 'unsafe-inline'").allows_inline(ResourceKind::Script));
        assert!(
            Csp::default().allows_inline(ResourceKind::Script),
            "a page with no policy is not restricted"
        );
    }

    #[test]
    fn a_nonce_is_recognised_as_a_source_we_cannot_evaluate() {
        // It must not be read as a host name, and it must not be mistaken for
        // 'unsafe-inline' — this engine cannot check a nonce, so inline script
        // stays blocked.
        let policy = csp("script-src 'nonce-abc123'");
        assert!(!policy.allows_inline(ResourceKind::Script));
        assert!(!policy.allows(
            "https://example.com/",
            "https://example.com/a.js",
            ResourceKind::Script
        ));
    }

    #[test]
    fn two_policies_both_apply_and_neither_can_widen_the_other() {
        // Sending a second policy is how a site tightens its rules; it can
        // never be a way to loosen them.
        let policy = Csp::parse(&[
            "img-src https://a.com".to_string(),
            "img-src https://b.com".to_string(),
        ]);
        let page = "https://example.com/";
        assert!(!policy.allows(page, "https://a.com/x.png", ResourceKind::Image));
        assert!(!policy.allows(page, "https://b.com/x.png", ResourceKind::Image));
    }

    #[test]
    fn a_page_with_no_policy_allows_everything() {
        let policy = Csp::default();
        assert!(policy.is_empty());
        assert!(policy.allows(
            "https://example.com/",
            "https://evil.com/x.js",
            ResourceKind::Script
        ));
    }

    #[test]
    fn non_fetch_directives_are_ignored_rather_than_treated_as_source_lists() {
        // `upgrade-insecure-requests` has no source list; reading it as one
        // would produce an empty list that blocks everything.
        let policy = csp("upgrade-insecure-requests; img-src *");
        assert!(policy.allows(
            "https://example.com/",
            "https://x.com/a.png",
            ResourceKind::Image
        ));
    }

    #[test]
    fn a_policy_in_a_meta_tag_is_found() {
        let html = r#"<html><head>
            <meta charset="utf-8">
            <meta http-equiv="Content-Security-Policy" content="default-src 'self'">
            </head><body></body></html>"#;
        assert_eq!(meta_csp(html), vec!["default-src 'self'".to_string()]);
    }

    #[test]
    fn a_meta_tag_that_is_not_a_policy_is_left_alone() {
        let html = r#"<meta http-equiv="refresh" content="5"><meta name="csp" content="x">"#;
        assert!(meta_csp(html).is_empty());
    }

    #[test]
    fn certificate_exceptions_are_per_host_and_not_granted_by_default() {
        assert!(!has_cert_exception("untrusted.example"));
        add_cert_exception("Untrusted.Example");
        assert!(
            has_cert_exception("untrusted.example"),
            "host matching ignores case"
        );
        assert!(
            !has_cert_exception("other.example"),
            "an exception covers one host, not the whole session"
        );
    }

    #[test]
    fn the_host_of_a_url_is_what_an_exception_is_keyed_on() {
        assert_eq!(
            host_of("https://untrusted.example:8443/page"),
            Some("untrusted.example".to_string())
        );
        assert_eq!(host_of("mistilteinn://bookmarks"), None);
    }

    #[test]
    fn a_certificate_failure_is_told_apart_from_other_connection_failures() {
        #[derive(Debug)]
        struct Fake(&'static str);
        impl std::fmt::Display for Fake {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for Fake {}

        assert!(is_certificate_error(&Fake(
            "invalid peer certificate: UnknownIssuer"
        )));
        assert!(is_certificate_error(&Fake("self-signed certificate")));
        assert!(!is_certificate_error(&Fake("dns error: name not resolved")));
    }
}
