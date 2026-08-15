use crate::css::Declaration;

use cssparser::{Parser, ParserInput, Token};

// ------ Selector Types ------

/// A single component of a selector (e.g., `div`, `.class`, `#id`, `[attr]`).
#[derive(Debug, Clone, PartialEq)]
pub enum SimpleSelector {
    /// Universal selector: `*`
    Universal,
    /// Type selector: `div`, `span`, `p`
    Type(String),
    /// Class selector: `.classname`
    Class(String),
    /// ID selector: `#element-id`
    Id(String),
    /// Attribute selector: `[attr]`, `[attr=value]`, `[attr~=value]`, etc.
    Attribute {
        name: String,
        operator: AttrOperator,
        value: Option<String>,
    },
    /// Pseudo-class: `:hover`, `:first-child`, `:nth-child(2n+1)`
    PseudoClass {
        name: String,
        arguments: Option<String>,
    },
    /// Pseudo-element: `::before`, `::after`, `::first-line`
    PseudoElement(String),
}

/// Attribute selector operator.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrOperator {
    /// `[attr]` — existence
    Existence,
    /// `[attr=value]` — exact match
    Exact,
    /// `[attr~=value]` — contains word
    Includes,
    /// `[attr|=value]` — starts with
    DashMatch,
    /// `[attr^=value]` — starts with (prefix)
    Prefix,
    /// `[attr$=value]` — ends with (suffix)
    Suffix,
    /// `[attr*=value]` — contains (substring)
    Substring,
}

/// A combinator joining two SimpleSelectors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Combinator {
    /// Descendant: `div span` (space)
    Descendant,
    /// Child: `div > span`
    Child,
    /// Adjacent sibling: `div + span`
    AdjacentSibling,
    /// General sibling: `div ~ span`
    GeneralSibling,
    /// Connects simple selectors within the same compound selector (e.g. `a.class`)
    Compound,
}

/// A full CSS selector: a chain of [`SimpleSelector`]s connected by [`Combinator`]s.
#[derive(Debug, Clone)]
pub struct Selector {
    /// Each entry is `(combinator, simple_selector)`.
    /// The first entry's combinator is always [`Combinator::Descendant`] (placeholder).
    pub complex: Vec<(Combinator, SimpleSelector)>,
}

/// Evaluate an attribute operator matching against an attribute value.
pub fn evaluate_attr_operator(
    attr_val: &str,
    operator: &AttrOperator,
    value: Option<&str>,
) -> bool {
    match (operator, value) {
        (AttrOperator::Existence, _) => true,
        (AttrOperator::Exact, Some(v)) => attr_val == v,
        (AttrOperator::Includes, Some(v)) => attr_val.split_whitespace().any(|w| w == v),
        (AttrOperator::DashMatch, Some(v)) => {
            attr_val == v || attr_val.starts_with(&format!("{}-", v))
        }
        (AttrOperator::Prefix, Some(v)) => attr_val.starts_with(v),
        (AttrOperator::Suffix, Some(v)) => attr_val.ends_with(v),
        (AttrOperator::Substring, Some(v)) => attr_val.contains(v),
        _ => false,
    }
}

/// Evaluate :nth-child(formula) against a 1-based index (e.g. "odd", "even", "2n+1", "3", "2n-1", "-n+4").
pub fn matches_nth_child(formula: &str, index_1based: usize) -> bool {
    let f = formula.trim().to_lowercase();
    if f == "odd" {
        return index_1based % 2 == 1;
    }
    if f == "even" {
        return index_1based % 2 == 0;
    }
    if let Ok(num) = f.parse::<usize>() {
        return index_1based == num;
    }
    let f = f.replace(' ', "");
    if let Some(n_pos) = f.find('n') {
        let a_str = &f[..n_pos];
        let a: i32 = if a_str.is_empty() || a_str == "+" {
            1
        } else if a_str == "-" {
            -1
        } else {
            a_str.parse().unwrap_or(1)
        };
        let b_str = &f[n_pos + 1..];
        let b: i32 = if b_str.is_empty() {
            0
        } else if b_str.starts_with('+') {
            b_str[1..].parse().unwrap_or(0)
        } else {
            b_str.parse().unwrap_or(0)
        };
        let idx = index_1based as i32;
        if a == 0 {
            return idx == b;
        }
        let diff = idx - b;
        if diff % a != 0 {
            return false;
        }
        let k = diff / a;
        k >= 0
    } else {
        false
    }
}

impl Selector {
    /// Create a new selector with just one simple selector.
    pub fn simple(sel: SimpleSelector) -> Self {
        Self {
            complex: vec![(Combinator::Descendant, sel)],
        }
    }

    /// Append a simple selector with a combinator.
    pub fn push(&mut self, combinator: Combinator, sel: SimpleSelector) {
        self.complex.push((combinator, sel));
    }

    /// Compute the CSS specificity of this selector as (id_count, class_count, type_count).
    ///
    /// Per CSS spec:
    /// - ID selectors (`#id`) contribute (1, 0, 0)
    /// - Class selectors (`.class`), attribute selectors (`[attr]`), and pseudo-classes (`:hover`) contribute (0, 1, 0)
    /// - Type selectors (`div`) and pseudo-elements (`::before`) contribute (0, 0, 1)
    /// - Universal selector (`*`) contributes (0, 0, 0)
    /// Specificity tuples compare lexicographically: IDs > classes > types.
    pub fn specificity(&self) -> (u32, u32, u32) {
        let mut ids = 0u32;
        let mut classes = 0u32;
        let mut types = 0u32;

        for (_, sel) in &self.complex {
            match sel {
                SimpleSelector::Id(_) => ids += 1,
                SimpleSelector::Class(_) => classes += 1,
                SimpleSelector::Attribute { .. } => classes += 1,
                SimpleSelector::PseudoClass { .. } => classes += 1,
                SimpleSelector::Type(_) => types += 1,
                SimpleSelector::PseudoElement(_) => types += 1,
                SimpleSelector::Universal => {}
            }
        }
        (ids, classes, types)
    }

    /// Check if this selector matches a DOM node (by tag/class/id lookup).
    ///
    /// Simplified — does not handle combinators fully (only checks the last
    /// simple-selector chain component).
    pub fn matches_element(
        &self,
        tag_name: &str,
        classes: impl Fn(&str) -> bool,
        has_id: impl Fn(&str) -> bool,
        is_first_child: impl Fn() -> bool,
    ) -> bool {
        if let Some((_, last)) = self.complex.last() {
            Self::simple_matches(last, tag_name, classes, has_id, is_first_child)
        } else {
            false
        }
    }

    fn simple_matches(
        sel: &SimpleSelector,
        tag_name: &str,
        classes: impl Fn(&str) -> bool,
        has_id: impl Fn(&str) -> bool,
        is_first_child: impl Fn() -> bool,
    ) -> bool {
        Self::simple_matches_with_context(
            sel,
            tag_name,
            classes,
            has_id,
            |_, _, _| false,
            is_first_child,
            || false,
            || false,
            || 1,
            || false,
        )
    }

    /// Like [`matches_element`] but also evaluates `:hover` at runtime.
    pub fn matches_element_with_hover(
        &self,
        tag_name: &str,
        classes: impl Fn(&str) -> bool,
        has_id: impl Fn(&str) -> bool,
        is_first_child: impl Fn() -> bool,
        is_hovered: impl Fn() -> bool,
    ) -> bool {
        if let Some((_, last)) = self.complex.last() {
            Self::simple_matches_with_hover(
                last,
                tag_name,
                classes,
                has_id,
                is_first_child,
                is_hovered,
            )
        } else {
            false
        }
    }

    pub fn simple_matches_with_hover(
        sel: &SimpleSelector,
        tag_name: &str,
        classes: impl Fn(&str) -> bool,
        has_id: impl Fn(&str) -> bool,
        is_first_child: impl Fn() -> bool,
        is_hovered: impl Fn() -> bool,
    ) -> bool {
        Self::simple_matches_with_context(
            sel,
            tag_name,
            classes,
            has_id,
            |_, _, _| false,
            is_first_child,
            || false,
            || false,
            || 1,
            is_hovered,
        )
    }

    /// Comprehensive selector matcher evaluating types, classes, IDs, attribute operators, and pseudo-classes.
    pub fn simple_matches_with_context(
        sel: &SimpleSelector,
        tag_name: &str,
        classes: impl Fn(&str) -> bool,
        has_id: impl Fn(&str) -> bool,
        matches_attr: impl Fn(&str, &AttrOperator, Option<&str>) -> bool,
        is_first_child: impl Fn() -> bool,
        is_last_child: impl Fn() -> bool,
        is_only_child: impl Fn() -> bool,
        child_index_1based: impl Fn() -> usize,
        is_hovered: impl Fn() -> bool,
    ) -> bool {
        match sel {
            SimpleSelector::Universal => true,
            SimpleSelector::Type(t) => tag_name.eq_ignore_ascii_case(t),
            SimpleSelector::Class(c) => classes(c),
            SimpleSelector::Id(i) => has_id(i),
            SimpleSelector::Attribute {
                name,
                operator,
                value,
            } => matches_attr(name, operator, value.as_deref()),
            SimpleSelector::PseudoClass { name, arguments } => match name.as_str() {
                "first-child" => is_first_child(),
                "last-child" => is_last_child(),
                "only-child" => is_only_child(),
                "nth-child" => {
                    if let Some(arg) = arguments {
                        matches_nth_child(arg, child_index_1based())
                    } else {
                        false
                    }
                }
                "hover" => is_hovered(),
                _ => false,
            },
            SimpleSelector::PseudoElement(_) => false,
        }
    }

    /// Recursively evaluate a complex selector against the DOM structure.
    pub fn full_matches(
        &self,
        node_id: u32,
        get_parent: &impl Fn(u32) -> Option<u32>,
        simple_match: &impl Fn(u32, &SimpleSelector) -> bool,
    ) -> bool {
        if self.complex.is_empty() {
            return false;
        }

        let mut current_node = node_id;
        let mut iter = self.complex.iter().rev();

        let &(mut target_combinator, ref last_sel) = iter.next().unwrap();
        if !simple_match(current_node, last_sel) {
            return false;
        }

        for (combinator, sel) in iter {
            match target_combinator {
                Combinator::Child => {
                    if let Some(parent_id) = get_parent(current_node) {
                        current_node = parent_id;
                        if !simple_match(current_node, sel) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                Combinator::Descendant => {
                    let mut matched = false;
                    while let Some(parent_id) = get_parent(current_node) {
                        current_node = parent_id;
                        if simple_match(current_node, sel) {
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        return false;
                    }
                }
                Combinator::Compound => {
                    // Same node
                    if !simple_match(current_node, sel) {
                        return false;
                    }
                }
                // Sibling combinators not fully supported in this simple engine
                _ => return false,
            }
            target_combinator = *combinator;
        }

        true
    }
}

// ------ CSS Rule ------

/// A single CSS rule: selector(s) + declaration block.
#[derive(Debug, Clone)]
pub struct CSSRule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

/// A CSS @import rule: points to an external stylesheet URL.
#[derive(Debug, Clone)]
pub struct ImportRule {
    pub url: String,
}

/// A parsed CSS media rule.
#[derive(Debug, Clone)]
pub struct MediaRule {
    pub condition: String,
    pub rules: Vec<CSSRule>,
}

/// Evaluates a media condition against a viewport width.
pub fn evaluate_media_condition(condition: &str, viewport_width: f32) -> bool {
    let mut matches = true;
    if let Some(min_idx) = condition.find("min-width:") {
        let rest = &condition[min_idx + 10..];
        let end_idx = rest.find(')').unwrap_or(rest.len());
        let val_str = rest[..end_idx].trim().trim_end_matches("px");
        if let Ok(min_w) = val_str.parse::<f32>() {
            if viewport_width < min_w {
                matches = false;
            }
        }
    }
    if let Some(max_idx) = condition.find("max-width:") {
        let rest = &condition[max_idx + 10..];
        let end_idx = rest.find(')').unwrap_or(rest.len());
        let val_str = rest[..end_idx].trim().trim_end_matches("px");
        if let Ok(max_w) = val_str.parse::<f32>() {
            if viewport_width > max_w {
                matches = false;
            }
        }
    }
    matches
}

/// A parsed CSS stylesheet containing rules.
#[derive(Debug, Clone)]
pub struct Stylesheet {
    pub rules: Vec<CSSRule>,
    pub imports: Vec<ImportRule>,
    pub media_rules: Vec<MediaRule>,
}

// ------ Helpers ------

/// Converts a cssparser CowRcStr to a String.
fn cow_to_string<T: std::ops::Deref<Target = str>>(cow: &T) -> String {
    cow.deref().to_string()
}

/// Converts a token to a CSS-readable string for value collection.
fn token_to_string(token: &Token<'_>) -> String {
    match token {
        Token::Ident(v) => cow_to_string(v),
        Token::Delim(c) => c.to_string(),
        Token::Number { value: v, .. } => v.to_string(),
        Token::Function(name) => format!("{}(", cow_to_string(name)),
        Token::QuotedString(s) => cow_to_string(s),
        Token::UnquotedUrl(u) => format!("url({})", cow_to_string(u)),
        Token::Dimension { value: v, unit, .. } => format!("{}{}", v, cow_to_string(unit)),
        Token::Hash(v) => format!("#{}", cow_to_string(v)),
        Token::IDHash(v) => format!("#{}", cow_to_string(v)),
        Token::Comma => ",".to_string(),
        Token::Colon => ":".to_string(),
        Token::Semicolon => ";".to_string(),
        Token::WhiteSpace(w) => w.to_string(),
        Token::Percentage { unit_value, .. } => format!("{:.1}%", unit_value * 100.0),
        Token::IncludeMatch => "~=".to_string(),
        Token::DashMatch => "|=".to_string(),
        Token::PrefixMatch => "^=".to_string(),
        Token::SuffixMatch => "$=".to_string(),
        Token::SubstringMatch => "*=".to_string(),
        _ => String::new(),
    }
}

// ------ @import Parsing ------

/// Tries to parse an `@import` rule from the prelude text (before `{` or `;`).
/// Handles:
/// - `@import "url.css";`
/// - `@import 'url.css';`
/// - `@import url("url.css");`
/// - `@import url(url.css);`
/// Returns `Some(ImportRule)` if the text is a valid @import directive.
fn try_parse_import(prelude: &str) -> Option<ImportRule> {
    let trimmed = prelude.trim();
    // Check for '@' followed by 'import' (case-insensitive)
    if !trimmed.starts_with('@') {
        return None;
    }
    let after_at = &trimmed[1..].trim_start();
    // Only compare the first 6 characters — the rest is the URL part
    if !after_at.starts_with("import")
        && !after_at.starts_with("IMPORT")
        && !after_at.starts_with("Import")
    {
        return None;
    }
    // Case-insensitive check for "import"
    let import_prefix: String = after_at.chars().take(6).collect();
    if !import_prefix.eq_ignore_ascii_case("import") {
        return None;
    }
    let after_import = if after_at.len() > 6 {
        &after_at[6..]
    } else {
        ""
    };
    let after_import = after_import.trim_start();

    // Try url(...) form
    if after_import.to_lowercase().starts_with("url(") {
        let rest = &after_import[4..];
        // Strip optional quotes inside url()
        let unquoted = rest.trim_start_matches('"').trim_start_matches('\'');
        let url_str = unquoted
            .strip_suffix(')')
            .unwrap_or(unquoted)
            .trim_end_matches('"')
            .trim_end_matches('\'')
            .to_string();
        if !url_str.is_empty() {
            return Some(ImportRule { url: url_str });
        }
    }

    // Try quoted string form: @import "..." or @import '...'
    let first_char = after_import.chars().next()?;
    if first_char == '"' || first_char == '\'' {
        let quote = first_char;
        let inner = &after_import[1..];
        if let Some(end) = inner.find(quote) {
            let url_str = inner[..end].to_string();
            if !url_str.is_empty() {
                return Some(ImportRule { url: url_str });
            }
        }
    }

    None
}

// ------ Stylesheet Parser ------

/// Parses a full CSS stylesheet into a [`Stylesheet`].
///
/// Uses string-based splitting to find `{ ... }` block pairs, then parses
/// selectors from prelude text using cssparser tokens, and declarations
/// from block text using the string-based parser. Also extracts `@import` rules.
pub fn parse_stylesheet(source: &str) -> Stylesheet {
    let mut rules = Vec::new();
    let mut imports = Vec::new();
    let mut media_rules = Vec::new();
    let mut pos = 0;

    while pos < source.len() {
        // Skip whitespace
        let rest = &source[pos..];
        let trimmed = rest.trim_start();
        pos += rest.len() - trimmed.len();
        if trimmed.is_empty() {
            break;
        }

        let starts_with_at = trimmed.chars().next() == Some('@');
        let open_brace_pos = find_matching_open_brace(trimmed);

        // Find the next semicolon, but only outside of strings/brackets if we wanted to be perfectly correct.
        // However, for this simple parser, we just find the first `;` and `{`.
        let semi_pos = trimmed.find(';');

        let is_block = match (open_brace_pos, semi_pos) {
            (Some(b), Some(s)) => b < s,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => {
                if starts_with_at {
                    if let Some(import_rule) = try_parse_import(trimmed) {
                        imports.push(import_rule);
                    }
                }
                break;
            }
        };

        if starts_with_at {
            if is_block {
                let brace_pos = open_brace_pos.unwrap();
                let prelude = trimmed[..brace_pos].trim();
                let block_start = brace_pos + 1;

                if let Some(block_end) = find_matching_close_brace(&trimmed[block_start..]) {
                    let block_text = &trimmed[block_start..block_start + block_end];

                    if prelude.starts_with("@media") {
                        let condition = prelude[6..].trim().to_string();
                        // Recursively parse the nested rules
                        let inner_stylesheet = parse_stylesheet(block_text);
                        media_rules.push(MediaRule {
                            condition,
                            rules: inner_stylesheet.rules,
                        });
                    }
                    pos += brace_pos + 1 + block_end + 1;
                } else {
                    break;
                }
            } else {
                let semi_pos = semi_pos.unwrap();
                let prelude = &trimmed[..semi_pos];
                if let Some(import_rule) = try_parse_import(prelude) {
                    imports.push(import_rule);
                }
                pos += semi_pos + 1;
            }
        } else {
            // Find the opening `{` for the block
            if let Some(brace_pos) = find_matching_open_brace(trimmed) {
                let selector_text = &trimmed[..brace_pos];
                // Find the matching `}`
                let block_start = brace_pos + 1; // skip `{`
                if let Some(block_end) = find_matching_close_brace(&trimmed[block_start..]) {
                    let decl_text = &trimmed[block_start..block_start + block_end];
                    let selectors = parse_selectors_from_string(selector_text);
                    let declarations = crate::css::parse_declarations(decl_text);
                    if !selectors.is_empty() {
                        rules.push(CSSRule {
                            selectors,
                            declarations,
                        });
                    }
                    pos += brace_pos + 1 + block_end + 1; // skip past `}`
                } else {
                    break;
                }
            } else {
                // No block found — skip to semicolon or end
                if let Some(semi_pos) = trimmed.find(';') {
                    pos += semi_pos + 1;
                } else {
                    break;
                }
            }
        }
    }

    Stylesheet {
        rules,
        imports,
        media_rules,
    }
}

/// Finds the position of the `{` that opens a declaration block.
/// This skips past any `@` rules or other structures.
fn find_matching_open_brace(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = 0;
    for ch in s.chars() {
        match ch {
            '{' => {
                if depth == 0 {
                    return Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {}
        }
        i += ch.len_utf8();
    }
    None
}

/// Finds the position of the matching `}` for a block starting at position 0.
fn find_matching_close_brace(s: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = 0;
    for ch in s.chars() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += ch.len_utf8();
    }
    None
}

fn collect_tokens_from_parser<'i, 't>(parser: &mut Parser<'i, 't>, tokens: &mut Vec<Token<'i>>) {
    loop {
        match parser.next_including_whitespace_and_comments() {
            Ok(t) => {
                let token = (*t).clone();
                match &token {
                    Token::SquareBracketBlock => {
                        tokens.push(Token::SquareBracketBlock);
                        let _ = parser.parse_nested_block(|nested| {
                            collect_tokens_from_parser(nested, tokens);
                            Ok::<(), cssparser::ParseError<'_, ()>>(())
                        });
                        tokens.push(Token::CloseSquareBracket);
                    }
                    Token::ParenthesisBlock | Token::Function(_) => {
                        tokens.push(token.clone());
                        let _ = parser.parse_nested_block(|nested| {
                            collect_tokens_from_parser(nested, tokens);
                            Ok::<(), cssparser::ParseError<'_, ()>>(())
                        });
                        tokens.push(Token::CloseParenthesis);
                    }
                    Token::BadUrl(_) | Token::BadString(_) | Token::Comment(_) => {}
                    _ => {
                        tokens.push(token);
                    }
                }
            }
            Err(_) => break,
        }
    }
}

/// Parses selectors from a CSS selector string using cssparser.
fn parse_selectors_from_string(selector_text: &str) -> Vec<Selector> {
    let trimmed = selector_text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut input = ParserInput::new(trimmed);
    let mut parser = Parser::new(&mut input);

    let mut tokens: Vec<Token<'_>> = Vec::new();
    collect_tokens_from_parser(&mut parser, &mut tokens);

    build_selectors_from_tokens(&tokens)
}

/// Builds selectors from collected prelude tokens.
fn build_selectors_from_tokens(tokens: &[Token<'_>]) -> Vec<Selector> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut selectors = Vec::new();
    let mut current_tokens: Vec<&Token<'_>> = Vec::new();

    for token in tokens.iter() {
        match token {
            Token::Comma => {
                if !current_tokens.is_empty() {
                    selectors.push(parse_single_selector_impl(&current_tokens));
                    current_tokens.clear();
                }
            }
            _ => {
                current_tokens.push(token);
            }
        }
    }

    if !current_tokens.is_empty() {
        selectors.push(parse_single_selector_impl(&current_tokens));
    }

    if selectors.is_empty() {
        selectors.push(Selector::simple(SimpleSelector::Universal));
    }

    selectors
}

/// Parses tokens of a single selector into a [`Selector`].
fn parse_single_selector_impl(tokens: &[&Token<'_>]) -> Selector {
    let mut parts: Vec<(Combinator, SimpleSelector)> = Vec::new();
    let mut i = 0;
    let mut pending_combinator: Option<Combinator> = None;

    while i < tokens.len() {
        let token = &tokens[i];

        if matches!(token, Token::WhiteSpace(_)) {
            i += 1;
            pending_combinator.get_or_insert(Combinator::Descendant);
            continue;
        }

        if let Some(comb) = try_parse_combinator(token) {
            pending_combinator = Some(comb);
            i += 1;
            continue;
        }

        let saved_comb = pending_combinator.take();
        if let Some(sel) = try_parse_simple_selector(token, tokens, &mut i) {
            let comb = saved_comb.unwrap_or_else(|| {
                if parts.is_empty() {
                    Combinator::Descendant // first placeholder
                } else {
                    Combinator::Compound
                }
            });
            parts.push((comb, sel));
            continue;
        }

        i += 1;
    }

    if parts.is_empty() {
        return Selector::simple(SimpleSelector::Universal);
    }

    Selector { complex: parts }
}

/// Tries to parse a [`SimpleSelector`] from the current token.
fn try_parse_simple_selector<'t>(
    token: &'t Token<'_>,
    tokens: &[&Token<'_>],
    i: &mut usize,
) -> Option<SimpleSelector> {
    match token {
        Token::Delim('*') => {
            *i += 1;
            Some(SimpleSelector::Universal)
        }
        Token::Hash(v) => {
            *i += 1;
            Some(SimpleSelector::Id(cow_to_string(v)))
        }
        Token::IDHash(v) => {
            *i += 1;
            Some(SimpleSelector::Id(cow_to_string(v)))
        }
        Token::Ident(v) => {
            *i += 1;
            Some(SimpleSelector::Type(cow_to_string(v)))
        }
        Token::Function(name) => {
            *i += 1;
            let mut args = String::new();
            let mut depth = 1usize;
            while *i < tokens.len() && depth > 0 {
                let t = &tokens[*i];
                if matches!(t, Token::CloseParenthesis) {
                    depth -= 1;
                } else if matches!(t, Token::ParenthesisBlock) {
                    depth += 1;
                } else if !matches!(t, Token::CloseCurlyBracket) {
                    args.push_str(&token_to_string(t));
                }
                *i += 1;
            }
            Some(SimpleSelector::PseudoClass {
                name: cow_to_string(name),
                arguments: if args.is_empty() { None } else { Some(args) },
            })
        }
        Token::SquareBracketBlock => {
            *i += 1;
            parse_attribute_selector(tokens, i)
        }
        Token::Colon => {
            *i += 1;
            if *i < tokens.len() && matches!(&tokens[*i], Token::Colon) {
                *i += 1;
                if let Token::Ident(name) =
                    tokens.get(*i).copied().unwrap_or(&Token::CloseCurlyBracket)
                {
                    *i += 1;
                    return Some(SimpleSelector::PseudoElement(cow_to_string(name)));
                }
            } else if *i < tokens.len() {
                match tokens[*i] {
                    Token::Ident(name) => {
                        *i += 1;
                        return Some(SimpleSelector::PseudoClass {
                            name: cow_to_string(name),
                            arguments: None,
                        });
                    }
                    Token::Function(name) => {
                        *i += 1;
                        let mut args = String::new();
                        let mut depth = 1usize;
                        while *i < tokens.len() && depth > 0 {
                            let t = &tokens[*i];
                            if matches!(t, Token::CloseParenthesis) {
                                depth -= 1;
                            } else if matches!(t, Token::ParenthesisBlock) {
                                depth += 1;
                            } else if !matches!(t, Token::CloseCurlyBracket) {
                                args.push_str(&token_to_string(t));
                            }
                            *i += 1;
                        }
                        return Some(SimpleSelector::PseudoClass {
                            name: cow_to_string(name),
                            arguments: if args.is_empty() { None } else { Some(args) },
                        });
                    }
                    _ => {}
                }
            }
            None
        }
        Token::Delim('.') => {
            *i += 1;
            if let Token::Ident(name) = tokens.get(*i).copied().unwrap_or(&Token::CloseCurlyBracket)
            {
                *i += 1;
                return Some(SimpleSelector::Class(cow_to_string(name)));
            }
            None
        }
        _ => None,
    }
}

/// Parses an attribute selector `[...]`.
fn parse_attribute_selector<'t>(tokens: &[&Token<'_>], i: &mut usize) -> Option<SimpleSelector> {
    let mut name = String::new();
    let mut operator = AttrOperator::Existence;
    let mut value = None;

    if *i < tokens.len() {
        match &tokens[*i] {
            Token::Ident(v) => name = cow_to_string(v),
            _ => name = token_to_string(&tokens[*i]),
        }
        *i += 1;
    }

    if *i < tokens.len() {
        match &tokens[*i] {
            Token::IncludeMatch => {
                operator = AttrOperator::Includes;
                *i += 1;
            }
            Token::DashMatch => {
                operator = AttrOperator::DashMatch;
                *i += 1;
            }
            Token::PrefixMatch => {
                operator = AttrOperator::Prefix;
                *i += 1;
            }
            Token::SuffixMatch => {
                operator = AttrOperator::Suffix;
                *i += 1;
            }
            Token::SubstringMatch => {
                operator = AttrOperator::Substring;
                *i += 1;
            }
            Token::Delim('=') => {
                operator = AttrOperator::Exact;
                *i += 1;
            }
            Token::CloseSquareBracket => {
                *i += 1;
                return Some(SimpleSelector::Attribute {
                    name,
                    operator: AttrOperator::Existence,
                    value: None,
                });
            }
            _ => {}
        }
    }

    if *i < tokens.len() && !matches!(&tokens[*i], Token::CloseSquareBracket) {
        value = Some(token_to_string(&tokens[*i]));
        *i += 1;
    }

    if *i < tokens.len() && matches!(&tokens[*i], Token::CloseSquareBracket) {
        *i += 1;
    }

    Some(SimpleSelector::Attribute {
        name,
        operator,
        value,
    })
}

/// Tries to parse a combinator from a token.
fn try_parse_combinator(token: &Token<'_>) -> Option<Combinator> {
    match token {
        Token::Delim('>') => Some(Combinator::Child),
        Token::Delim('+') => Some(Combinator::AdjacentSibling),
        Token::Delim('~') => Some(Combinator::GeneralSibling),
        _ => None,
    }
}

// ------ Convenience ------

/// Parses a single CSS selector string.
pub fn parse_single_selector(source: &str) -> Selector {
    let stylesheet = parse_stylesheet(&format!("{} {{ color: red }}", source.trim()));
    if let Some(rule) = stylesheet.rules.first() {
        if let Some(sel) = rule.selectors.first() {
            return sel.clone();
        }
    }
    Selector::simple(SimpleSelector::Type(source.trim().to_string()))
}

/// Parses a selector string into components (for testing).
pub fn parse_selector_str(source: &str) -> Vec<Selector> {
    let stylesheet = parse_stylesheet(&format!("{} {{ color: red }}", source.trim()));
    if let Some(rule) = stylesheet.rules.first() {
        return rule.selectors.clone();
    }
    vec![Selector::simple(SimpleSelector::Type(
        source.trim().to_string(),
    ))]
}

// ------ Tests ------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_stylesheet() {
        let ss = parse_stylesheet("");
        assert_eq!(ss.rules.len(), 0);
    }

    #[test]
    fn parse_single_rule() {
        let ss = parse_stylesheet("div { color: red; margin: 10px }");
        assert_eq!(ss.rules.len(), 1);
        assert!(!ss.rules[0].selectors.is_empty());
        assert_eq!(ss.rules[0].declarations.len(), 2);
    }

    #[test]
    fn parse_multiple_rules() {
        let ss = parse_stylesheet("div { color: red } span { font-size: 14px }");
        assert_eq!(ss.rules.len(), 2);
    }

    #[test]
    fn parse_declaration_values() {
        let ss = parse_stylesheet("p { color: blue; background: #fff }");
        let rule = &ss.rules[0];
        assert_eq!(rule.declarations.len(), 2);
        assert_eq!(rule.declarations[0].property, "color");
        assert_eq!(rule.declarations[0].value, "blue");
        assert_eq!(rule.declarations[1].value, "#fff");
    }

    #[test]
    fn parse_important_declaration() {
        let ss = parse_stylesheet("p { color: red !important }");
        let rule = &ss.rules[0];
        assert!(rule.declarations[0].important);
    }

    #[test]
    fn parse_type_selector() {
        let ss = parse_stylesheet("div { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(
            sel.complex
                .iter()
                .any(|(_, s)| matches!(s, SimpleSelector::Type(t) if t == "div"))
        );
    }

    #[test]
    fn parse_id_selector() {
        let ss = parse_stylesheet("#header { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(
            sel.complex
                .iter()
                .any(|(_, s)| matches!(s, SimpleSelector::Id(i) if i == "header"))
        );
    }

    #[test]
    fn parse_universal_selector() {
        let ss = parse_stylesheet("* { margin: 0 }");
        assert!(!ss.rules.is_empty());
        let sel = &ss.rules[0].selectors[0];
        assert!(
            sel.complex
                .iter()
                .any(|(_, s)| matches!(s, SimpleSelector::Universal))
        );
    }

    #[test]
    fn parse_child_combinator() {
        let ss = parse_stylesheet("div > span { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(sel.complex.iter().any(|(c, _)| *c == Combinator::Child));
    }

    #[test]
    fn parse_adjacent_sibling_combinator() {
        let ss = parse_stylesheet("div + span { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(
            sel.complex
                .iter()
                .any(|(c, _)| *c == Combinator::AdjacentSibling)
        );
    }

    #[test]
    fn parse_selector_matching() {
        let sel = Selector::simple(SimpleSelector::Type("div".to_string()));
        assert!(sel.matches_element("div", |_| false, |_| false, || false));
        assert!(!sel.matches_element("span", |_| false, |_| false, || false));
    }

    #[test]
    fn parse_class_matching() {
        let sel = Selector::simple(SimpleSelector::Class("btn".to_string()));
        assert!(sel.matches_element("div", |c| c == "btn", |_| false, || false));
    }

    #[test]
    fn parse_id_matching() {
        let sel = Selector::simple(SimpleSelector::Id("header".to_string()));
        assert!(sel.matches_element("div", |_| false, |i| i == "header", || false));
    }

    #[test]
    fn parse_universal_matching() {
        let sel = Selector::simple(SimpleSelector::Universal);
        assert!(sel.matches_element("anything", |_| false, |_| false, || false));
    }

    #[test]
    fn parse_pseudo_class() {
        let ss = parse_stylesheet("a:hover { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(sel.complex.iter().any(
            |(_, s)| matches!(s, SimpleSelector::PseudoClass { name, .. } if name == "hover")
        ));
    }

    #[test]
    fn parse_attribute_selector() {
        let ss = parse_stylesheet("[href] { color: blue }");
        let sel = &ss.rules[0].selectors[0];
        assert!(
            sel.complex
                .iter()
                .any(|(_, s)| matches!(s, SimpleSelector::Attribute { .. }))
        );
    }

    #[test]
    fn parse_comma_selector_list() {
        let ss = parse_stylesheet("h1, h2, h3 { color: red }");
        let rule = &ss.rules[0];
        assert_eq!(rule.selectors.len(), 3);
    }

    // -- Pseudo-class matching behavior tests --

    #[test]
    fn hover_does_not_match_static() {
        // :hover should NOT match during static (non-interactive) style computation.
        // It requires runtime mouse position context to evaluate.
        let ss = parse_stylesheet("a:hover { color: red }");
        let sel = &ss.rules[0].selectors[0];
        // :hover is the last simple selector in the chain (after `a` type)
        assert!(sel.complex.iter().any(
            |(_, s)| matches!(s, SimpleSelector::PseudoClass { name, .. } if name == "hover")
        ));
        // During static computation, :hover should not match
        assert!(!sel.matches_element("a", |_| false, |_| false, || false));
        // Even if the element is a first child, :hover still doesn't match statically
        assert!(!sel.matches_element("a", |_| false, |_| false, || true));
    }

    #[test]
    fn first_child_matches_when_true() {
        let ss = parse_stylesheet("li:first-child { font-weight: bold }");
        let sel = &ss.rules[0].selectors[0];
        assert!(sel.matches_element("li", |_| false, |_| false, || true));
    }

    #[test]
    fn first_child_does_not_match_when_false() {
        let ss = parse_stylesheet("li:first-child { font-weight: bold }");
        let sel = &ss.rules[0].selectors[0];
        assert!(!sel.matches_element("li", |_| false, |_| false, || false));
    }

    // -- Hover with dynamic context tests --

    #[test]
    fn hover_matches_with_hover_context() {
        // :hover should match when the is_hovered closure returns true
        let ss = parse_stylesheet("a:hover { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(sel.matches_element_with_hover("a", |_| false, |_| false, || false, || true));
    }

    #[test]
    fn hover_does_not_match_without_hover_context() {
        // :hover should NOT match when the is_hovered closure returns false
        let ss = parse_stylesheet("a:hover { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert!(!sel.matches_element_with_hover("a", |_| false, |_| false, || false, || false));
    }

    #[test]
    fn hover_class_combo_matches_when_hovered() {
        // NOTE: The parser only checks the last component of a selector chain.
        // For `a.special:hover`, only `:hover` is checked (last component).
        // This is consistent with existing matches_element behavior.

        // :hover on its own should match when hovered
        let ss = parse_stylesheet("a:hover { color: blue }");
        let sel = &ss.rules[0].selectors[0];
        assert!(sel.matches_element_with_hover("a", |_| false, |_| false, || false, || true));
        // Should NOT match when not hovered
        assert!(!sel.matches_element_with_hover("a", |_| false, |_| false, || false, || false));

        // Test :hover with class on a different tag type
        let ss2 = parse_stylesheet(".special:hover { color: green }");
        let sel2 = &ss2.rules[0].selectors[0];
        // Only :hover (last) is checked, so it matches based on hover state only
        assert!(sel2.matches_element_with_hover("div", |_| false, |_| false, || false, || true));
    }

    // -- Color tests --

    #[test]
    fn parse_color_red() {
        assert_eq!(
            crate::css::parse_color_value("red"),
            Some(crate::css::CSSColor::Named("red".to_string()))
        );
    }

    #[test]
    fn parse_color_hex() {
        assert_eq!(
            crate::css::parse_color_value("#ff0000"),
            Some(crate::css::CSSColor::Hex { r: 255, g: 0, b: 0 })
        );
    }

    #[test]
    fn parse_color_hex_short() {
        assert_eq!(
            crate::css::parse_color_value("#f00"),
            Some(crate::css::CSSColor::Hex { r: 255, g: 0, b: 0 })
        );
    }

    #[test]
    fn parse_color_invalid() {
        assert!(crate::css::parse_color_value("invalid").is_none());
    }

    // -- @import parsing tests --

    #[test]
    fn parse_import_double_quotes() {
        let ss = parse_stylesheet("@import \"styles.css\";");
        assert_eq!(ss.imports.len(), 1);
        assert_eq!(ss.imports[0].url, "styles.css");
    }

    #[test]
    fn parse_import_url_function() {
        let ss = parse_stylesheet("@import url(\"theme.css\");");
        assert_eq!(ss.imports.len(), 1);
        assert_eq!(ss.imports[0].url, "theme.css");
    }

    #[test]
    fn parse_import_single_quotes() {
        let ss = parse_stylesheet("@import 'base.css';");
        assert_eq!(ss.imports.len(), 1);
        assert_eq!(ss.imports[0].url, "base.css");
    }

    #[test]
    fn parse_import_absolute_url() {
        let ss = parse_stylesheet("@import url(https://cdn.example.com/lib.css);");
        assert_eq!(ss.imports.len(), 1);
        assert_eq!(ss.imports[0].url, "https://cdn.example.com/lib.css");
    }

    #[test]
    fn parse_multiple_imports_with_rules() {
        let ss = parse_stylesheet(
            "@import \"a.css\"; div { color: red; } @import url(\"b.css\"); span { margin: 0; }",
        );
        assert_eq!(ss.imports.len(), 2);
        assert_eq!(ss.imports[0].url, "a.css");
        assert_eq!(ss.imports[1].url, "b.css");
        assert_eq!(ss.rules.len(), 2);
    }

    #[test]
    fn parse_import_interspersed_with_rules() {
        let ss = parse_stylesheet("p { margin: 1em; } @import \"mid.css\"; h1 { font-size: 2em; }");
        assert_eq!(ss.imports.len(), 1);
        assert_eq!(ss.imports[0].url, "mid.css");
        assert_eq!(ss.rules.len(), 2);
    }

    #[test]
    fn parse_no_imports_empty_vector() {
        let ss = parse_stylesheet("div { color: red; } span { display: block; }");
        assert_eq!(ss.imports.len(), 0);
        assert_eq!(ss.rules.len(), 2);
    }

    #[test]
    fn parse_import_url_function_unquoted() {
        let ss = parse_stylesheet("@import url(base.css);");
        assert_eq!(ss.imports.len(), 1);
        assert_eq!(ss.imports[0].url, "base.css");
    }
}
