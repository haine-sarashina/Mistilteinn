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

/// Everything the matcher may ask about the one element under test.
///
/// The fields are trait objects rather than generics so that `:not(...)` can
/// call back into the matcher without the compiler chasing an unbounded chain
/// of instantiations.
struct MatchCtx<'a> {
    tag_name: &'a str,
    classes: &'a dyn Fn(&str) -> bool,
    has_id: &'a dyn Fn(&str) -> bool,
    matches_attr: &'a dyn Fn(&str, &AttrOperator, Option<&str>) -> bool,
    is_first_child: &'a dyn Fn() -> bool,
    is_last_child: &'a dyn Fn() -> bool,
    is_only_child: &'a dyn Fn() -> bool,
    child_index_1based: &'a dyn Fn() -> usize,
    is_hovered: &'a dyn Fn() -> bool,
}

/// Match one simple selector against the element `ctx` describes.
fn matches_simple(sel: &SimpleSelector, ctx: &MatchCtx<'_>) -> bool {
    match sel {
        SimpleSelector::Universal => true,
        SimpleSelector::Type(t) => ctx.tag_name.eq_ignore_ascii_case(t),
        SimpleSelector::Class(c) => (ctx.classes)(c),
        SimpleSelector::Id(i) => (ctx.has_id)(i),
        SimpleSelector::Attribute {
            name,
            operator,
            value,
        } => (ctx.matches_attr)(name, operator, value.as_deref()),
        SimpleSelector::PseudoClass { name, arguments } => match name.as_str() {
            "first-child" => (ctx.is_first_child)(),
            "last-child" => (ctx.is_last_child)(),
            "only-child" => (ctx.is_only_child)(),
            "nth-child" => arguments
                .as_deref()
                .is_some_and(|arg| matches_nth_child(arg, (ctx.child_index_1based)())),
            "hover" => (ctx.is_hovered)(),
            "not" => arguments
                .as_deref()
                .is_some_and(|args| !nested_list_matches(args, ctx)),
            "is" | "where" | "matches" | "any" | "-moz-any" | "-webkit-any" => arguments
                .as_deref()
                .is_some_and(|args| nested_list_matches(args, ctx)),
            _ => false,
        },
        SimpleSelector::PseudoElement(_) => false,
    }
}

/// Whether any selector in a nested list matches the element under test.
///
/// Combinators inside the list are not walked: each selector is judged by its
/// own simple parts against this one element. `:not(.a .b)` therefore asks
/// whether the element is both at once, which is stricter than the descendant
/// test it stands for — and a `:not()` that is too strict lets its rule
/// through rather than dropping it, which is the safer way to be wrong.
fn nested_list_matches(arguments: &str, ctx: &MatchCtx<'_>) -> bool {
    nested_selectors(arguments).iter().any(|sel| {
        !sel.complex.is_empty()
            && sel
                .complex
                .iter()
                .all(|(_, simple)| matches_simple(simple, ctx))
    })
}

/// The most specific selector in a nested list, for `:not()` and `:is()`.
fn nested_specificity(arguments: &str) -> (u32, u32, u32) {
    nested_selectors(arguments)
        .iter()
        .map(|sel| sel.specificity())
        .max()
        .unwrap_or((0, 0, 0))
}

thread_local! {
    /// Parsed `:not()` / `:is()` / `:where()` arguments, keyed by the text
    /// inside the parentheses.
    ///
    /// The cascade asks about the same handful of arguments once per element,
    /// and a page has far more elements than it has distinct selectors.
    static NESTED_SELECTORS: std::cell::RefCell<
        rustc_hash::FxHashMap<String, std::rc::Rc<Vec<Selector>>>,
    > = std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

/// The argument of a nested-selector pseudo-class, parsed once and kept.
fn nested_selectors(arguments: &str) -> std::rc::Rc<Vec<Selector>> {
    NESTED_SELECTORS.with(|cache| {
        if let Some(parsed) = cache.borrow().get(arguments) {
            return parsed.clone();
        }
        let parsed = std::rc::Rc::new(parse_selectors_from_string(arguments));
        cache
            .borrow_mut()
            .insert(arguments.to_string(), parsed.clone());
        parsed
    })
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
                SimpleSelector::PseudoClass { name, arguments } => match name.as_str() {
                    // `:where()` adds nothing, which is the whole point of it.
                    "where" => {}
                    // `:not()` and `:is()` count as their most specific argument.
                    "not" | "is" | "matches" => {
                        if let Some(args) = arguments {
                            let (i, c, t) = nested_specificity(args);
                            ids += i;
                            classes += c;
                            types += t;
                        }
                    }
                    _ => classes += 1,
                },
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
        // Every caller brings its own closure types, but `:not()` has to
        // re-enter the matcher on the same element. Trait objects give it one
        // type to recurse through.
        matches_simple(
            sel,
            &MatchCtx {
                tag_name,
                classes: &classes,
                has_id: &has_id,
                matches_attr: &matches_attr,
                is_first_child: &is_first_child,
                is_last_child: &is_last_child,
                is_only_child: &is_only_child,
                child_index_1based: &child_index_1based,
                is_hovered: &is_hovered,
            },
        )
    }

    /// The pseudo-element this selector targets, if it ends in one.
    ///
    /// `p::before` selects a box that belongs to a `p` but is not the `p`
    /// itself, so it is matched in two parts: this says which box, and
    /// [`Selector::without_pseudo_element`] gives the selector that finds the
    /// element it hangs off.
    pub fn pseudo_element(&self) -> Option<&str> {
        match self.complex.last() {
            Some((_, SimpleSelector::PseudoElement(name))) => Some(name.as_str()),
            _ => None,
        }
    }

    /// This selector with its trailing pseudo-element removed.
    pub fn without_pseudo_element(&self) -> Selector {
        let mut complex = self.complex.clone();
        if matches!(complex.last(), Some((_, SimpleSelector::PseudoElement(_)))) {
            complex.pop();
        }
        // `::before` on its own means `*::before`: the element part is empty,
        // so it has to match every element rather than none.
        if complex.is_empty() {
            complex.push((Combinator::Descendant, SimpleSelector::Universal));
        }
        Selector { complex }
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
    /// Where this rule sat in the document, counting rules nested inside
    /// `@media` and `@supports` alongside the plain ones.
    ///
    /// The cascade breaks a specificity tie by taking the later rule, so it
    /// needs to know which one that was. Grouping the conditional rules at the
    /// end of the list instead handed every `@media` rule an undeserved win.
    pub order: usize,
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

/// Whether a media query list matches the screen we are rendering to.
///
/// `condition` is everything after `@media`, or the value of a `<link>`'s
/// `media` attribute — a comma-separated list of queries, matching if any one
/// of them does.
///
/// Two rules matter more than the feature list:
///
/// * The **media type** is checked. Without that, `@media print` matched, and
///   a page's print stylesheet — which is written to strip navigation and swap
///   in a serif face — landed on top of its screen styles.
/// * A feature whose value we cannot parse makes its query **false**, never
///   true. CSS drops what it cannot understand; a query that fails open
///   applies a `max-width: 640px` block to a 1280px window.
pub fn evaluate_media_condition(condition: &str, viewport_width: f32) -> bool {
    let condition = condition.trim();
    // `@media { … }` with nothing to say applies everywhere.
    if condition.is_empty() {
        return true;
    }
    split_top_level(condition, ',')
        .iter()
        .any(|query| evaluate_media_query(query, viewport_width))
}

/// One query out of a comma-separated list.
///
/// Shape: `[not | only] <media-type> [and <feature>]*`, or a bare list of
/// features with the type left implicit (`all`).
fn evaluate_media_query(query: &str, viewport_width: f32) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return false;
    }

    // `not` negates the whole query; `only` exists to hide a query from CSS2
    // parsers and means nothing to us.
    let (negated, rest) = match strip_leading_keyword(&query, "not") {
        Some(rest) => (true, rest),
        None => (
            false,
            strip_leading_keyword(&query, "only").unwrap_or(&query),
        ),
    };

    let matched = split_top_level_and(rest)
        .iter()
        .all(|part| evaluate_media_part(part, viewport_width));

    matched != negated
}

/// One `and`-joined term: either a media type or a parenthesised feature.
fn evaluate_media_part(part: &str, viewport_width: f32) -> bool {
    let part = part.trim();
    if part.is_empty() {
        return false;
    }
    if part.starts_with('(') {
        evaluate_media_feature(part, viewport_width)
    } else {
        // We are a screen. `print`, `speech` and the deprecated types are not us.
        matches!(part, "all" | "screen")
    }
}

/// A single `(feature: value)` term.
///
/// An unrecognised feature is false, per the CSS rule that an unknown media
/// feature never matches.
fn evaluate_media_feature(feature: &str, viewport_width: f32) -> bool {
    let inner = feature
        .trim()
        .strip_prefix('(')
        .and_then(|f| f.strip_suffix(')'))
        .unwrap_or("")
        .trim();
    if inner.is_empty() {
        return false;
    }

    // A parenthesised group rather than a feature: `((a) and (b))`.
    if inner.starts_with('(') {
        return evaluate_media_query(inner, viewport_width);
    }

    let Some((name, value)) = inner.split_once(':') else {
        // A boolean feature — `(color)`, `(hover)`. None of the ones we could
        // answer for are worth guessing at.
        return false;
    };
    let name = name.trim();
    let value = value.trim();

    // Lengths here may be arithmetic: MediaWiki writes `calc(640px - 1px)`.
    let length = || {
        crate::css::parse_length_ctx(
            value,
            crate::css::LengthContext {
                viewport_width,
                ..crate::css::LengthContext::default()
            },
        )
    };

    match name {
        "min-width" => length().is_some_and(|v| viewport_width >= v),
        "max-width" => length().is_some_and(|v| viewport_width <= v),
        "width" => length().is_some_and(|v| (viewport_width - v).abs() < 0.5),
        // We paint a light page and animate freely.
        "prefers-color-scheme" => value == "light",
        "prefers-reduced-motion" => value == "no-preference",
        // A desktop window with a mouse in it.
        "hover" | "any-hover" => value == "hover",
        "pointer" | "any-pointer" => value == "fine",
        "orientation" => value == "landscape",
        _ => false,
    }
}

/// `keyword` if the string starts with it as a whole word, and the rest after it.
fn strip_leading_keyword<'a>(s: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(keyword)?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()) {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Split on the word `and` where it is not inside parentheses.
fn split_top_level_and(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && bytes[i..].starts_with(b"and") {
            let before_ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
            let after = i + 3;
            let after_ok = after >= bytes.len() || bytes[after].is_ascii_whitespace();
            if before_ok && after_ok {
                parts.push(s[start..i].trim());
                start = after;
                i = after;
                continue;
            }
        }
        i += 1;
    }
    parts.push(s[start..].trim());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// Whether an `@supports` condition holds for this engine.
///
/// The grammar is `not X`, `X and Y`, `X or Y`, parentheses, and the leaf
/// `(property: value)`. A leaf we cannot read at all is false, which is the
/// answer that makes a page hand us its fallback rather than the branch
/// written for a browser we are not.
pub fn evaluate_supports_condition(condition: &str) -> bool {
    let cond = condition.trim();
    if cond.is_empty() {
        return false;
    }

    // `or` binds loosest, then `and`, then `not`.
    let ors = split_top_level_keyword(cond, "or");
    if ors.len() > 1 {
        return ors.iter().any(|part| evaluate_supports_condition(part));
    }
    let ands = split_top_level_keyword(cond, "and");
    if ands.len() > 1 {
        return ands.iter().all(|part| evaluate_supports_condition(part));
    }

    if let Some(rest) = strip_leading_keyword(cond, "not") {
        return !evaluate_supports_condition(rest);
    }

    // `selector(...)`, `font-format(...)` and friends: we make no claim.
    if !cond.starts_with('(') {
        return false;
    }

    let Some(inner) = cond.strip_prefix('(').and_then(|c| c.strip_suffix(')')) else {
        return false;
    };
    let inner = inner.trim();

    // A parenthesised group rather than a declaration.
    if inner.starts_with('(') || starts_with_keyword(inner, "not") {
        return evaluate_supports_condition(inner);
    }

    match inner.split_once(':') {
        Some((property, value)) => crate::css::supports_declaration(property, value),
        None => false,
    }
}

/// Whether the string opens with `keyword` as a whole word.
fn starts_with_keyword(s: &str, keyword: &str) -> bool {
    strip_leading_keyword(s, keyword).is_some()
}

/// Split on `keyword` used as a whole word outside any parentheses.
fn split_top_level_keyword<'a>(s: &'a str, keyword: &str) -> Vec<&'a str> {
    let bytes = s.as_bytes();
    let kw = keyword.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && bytes[i..].len() >= kw.len() && bytes[i..].starts_with(kw) {
            let before_ok = i > 0 && bytes[i - 1].is_ascii_whitespace();
            let after = i + kw.len();
            let after_ok = after < bytes.len() && bytes[after].is_ascii_whitespace();
            if before_ok && after_ok {
                parts.push(s[start..i].trim());
                start = after;
                i = after;
                continue;
            }
        }
        i += 1;
    }
    parts.push(s[start..].trim());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// Where one `@font-face` source points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontFaceSource {
    /// A font file to download.
    Url {
        url: String,
        /// The declared `format(...)`, lowercased; `None` when omitted.
        format: Option<String>,
    },
    /// A font already installed on this machine.
    Local(String),
}

/// A parsed `@font-face` rule.
///
/// Only the family and its sources are kept. The `font-weight` and
/// `font-style` descriptors, which let one family name cover several files,
/// are not modelled — every source is registered under the family name and
/// the shaper picks between them by the font's own metadata.
#[derive(Debug, Clone)]
pub struct FontFaceRule {
    pub family: String,
    pub sources: Vec<FontFaceSource>,
}

/// A parsed CSS stylesheet containing rules.
#[derive(Debug, Clone)]
pub struct Stylesheet {
    pub rules: Vec<CSSRule>,
    pub imports: Vec<ImportRule>,
    pub media_rules: Vec<MediaRule>,
    pub font_faces: Vec<FontFaceRule>,
    /// `@keyframes` rules, in source order. Looked up by name when an element
    /// says `animation-name`.
    pub keyframes: Vec<crate::css::KeyframesRule>,
}

/// Parse the body of an `@font-face` block.
///
/// Returns `None` unless the rule names a family and offers at least one
/// source — a rule missing either cannot load anything.
pub fn parse_font_face(block: &str) -> Option<FontFaceRule> {
    let mut family = None;
    let mut sources = Vec::new();

    for decl in crate::css::parse_declarations(block) {
        match decl.property.to_ascii_lowercase().as_str() {
            "font-family" => {
                let name = decl
                    .value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .trim();
                if !name.is_empty() {
                    family = Some(name.to_string());
                }
            }
            "src" => sources.extend(parse_font_face_src(&decl.value)),
            _ => {}
        }
    }

    let family = family?;
    if sources.is_empty() {
        return None;
    }
    Some(FontFaceRule { family, sources })
}

/// Parse the comma-separated `src:` list of an `@font-face` rule.
fn parse_font_face_src(value: &str) -> Vec<FontFaceSource> {
    let mut sources = Vec::new();

    for entry in split_top_level(value, ',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        // Each entry is `url(...) format(...)` or `local(...)`; the format hint
        // is optional and may be followed by nothing else we care about.
        let mut url = None;
        let mut format = None;
        let mut local = None;

        for token in split_top_level(entry, ' ') {
            let token = token.trim();
            if let Some(inner) = strip_css_function(token, "url") {
                url = Some(unquote(inner).to_string());
            } else if let Some(inner) = strip_css_function(token, "format") {
                format = Some(unquote(inner).to_ascii_lowercase());
            } else if let Some(inner) = strip_css_function(token, "local") {
                local = Some(unquote(inner).to_string());
            }
        }

        if let Some(url) = url {
            if !url.is_empty() {
                sources.push(FontFaceSource::Url { url, format });
            }
        } else if let Some(local) = local {
            if !local.is_empty() {
                sources.push(FontFaceSource::Local(local));
            }
        }
    }

    sources
}

/// Split on `sep`, ignoring separators nested inside parentheses or quotes.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut start = 0usize;

    for (i, c) in s.char_indices() {
        match c {
            '"' | '\'' if quote == Some(c) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(c),
            '(' if quote.is_none() => depth += 1,
            ')' if quote.is_none() => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 && quote.is_none() => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Strip a `name(...)` wrapper, case-insensitively on the name.
fn strip_css_function<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    let prefix_len = name.len() + 1;
    if s.len() < prefix_len + 1 || !s.ends_with(')') {
        return None;
    }
    if !s[..name.len()].eq_ignore_ascii_case(name) || !s[name.len()..].starts_with('(') {
        return None;
    }
    Some(&s[prefix_len..s.len() - 1])
}

/// Remove one layer of matching quotes.
fn unquote(s: &str) -> &str {
    s.trim().trim_matches(|c| c == '"' || c == '\'').trim()
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
/// Slide a nested sheet's rule numbering to start at `base`, and report the
/// next free number.
///
/// A nested `parse_stylesheet` numbers from zero because it cannot see what
/// came before it; this puts the block back where it belongs in the document.
fn continue_order_from(sheet: &mut Stylesheet, base: usize) -> usize {
    let mut next = base;
    let nested = sheet
        .media_rules
        .iter_mut()
        .flat_map(|media| media.rules.iter_mut());
    for rule in sheet.rules.iter_mut().chain(nested) {
        rule.order += base;
        next = next.max(rule.order + 1);
    }
    next
}

pub fn parse_stylesheet(source: &str) -> Stylesheet {
    let mut rules = Vec::new();
    let mut imports = Vec::new();
    let mut media_rules = Vec::new();
    let mut font_faces = Vec::new();
    let mut keyframes = Vec::new();
    let mut pos = 0;
    let mut order = 0usize;

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
                        let mut inner_stylesheet = parse_stylesheet(block_text);
                        order = continue_order_from(&mut inner_stylesheet, order);
                        media_rules.push(MediaRule {
                            condition,
                            rules: inner_stylesheet.rules,
                        });
                        font_faces.extend(inner_stylesheet.font_faces);
                        keyframes.extend(inner_stylesheet.keyframes);
                    } else if let Some(supports) = prelude.strip_prefix("@supports") {
                        // The condition does not depend on the viewport, so it
                        // is settled here and the block either becomes ordinary
                        // rules or disappears. Skipping every `@supports`
                        // wholesale dropped 83 blocks of ja.wikipedia.org's
                        // layout on the floor.
                        if evaluate_supports_condition(supports) {
                            let mut inner_stylesheet = parse_stylesheet(block_text);
                            order = continue_order_from(&mut inner_stylesheet, order);
                            rules.extend(inner_stylesheet.rules);
                            media_rules.extend(inner_stylesheet.media_rules);
                            font_faces.extend(inner_stylesheet.font_faces);
                            keyframes.extend(inner_stylesheet.keyframes);
                        }
                    } else if prelude
                        .get(..10)
                        .is_some_and(|p| p.eq_ignore_ascii_case("@font-face"))
                    {
                        font_faces.extend(parse_font_face(block_text));
                    } else if let Some(name) = keyframes_name(prelude) {
                        keyframes.extend(crate::css::animation::parse_keyframes_block(
                            &name, block_text,
                        ));
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
                            order,
                        });
                        order += 1;
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
        font_faces,
        keyframes,
    }
}

/// The name an `@keyframes` prelude declares, vendor prefix and all.
fn keyframes_name(prelude: &str) -> Option<String> {
    let prelude = prelude.trim();
    let rest = prelude
        .strip_prefix("@keyframes")
        .or_else(|| prelude.strip_prefix("@-webkit-keyframes"))
        .or_else(|| prelude.strip_prefix("@-moz-keyframes"))?;
    let name = rest.trim();
    (!name.is_empty()).then(|| name.to_string())
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

/// Read the text between a pseudo-class function's parentheses.
///
/// `*i` starts just past the `Function` token and ends just past its closing
/// parenthesis. The text is rebuilt so it can be parsed again — `:not()` and
/// `:is()` hold a selector list, and a list that came back as
/// `role=button` instead of `[role="button"]` would match nothing.
fn collect_function_arguments(tokens: &[&Token<'_>], i: &mut usize) -> Option<String> {
    let mut args = String::new();
    let mut depth = 1usize;
    while *i < tokens.len() && depth > 0 {
        let t = &tokens[*i];
        // A `Function` token opens a block of its own, exactly as
        // `ParenthesisBlock` does — counting only the latter used to end the
        // argument early on `:where(.new:not(…))`.
        if matches!(t, Token::CloseParenthesis) {
            depth -= 1;
            if depth > 0 {
                args.push(')');
            }
        } else if matches!(t, Token::ParenthesisBlock | Token::Function(_)) {
            depth += 1;
            args.push_str(&token_to_selector_text(t));
        } else if !matches!(t, Token::CloseCurlyBracket) {
            args.push_str(&token_to_selector_text(t));
        }
        *i += 1;
    }
    let args = args.trim().to_string();
    (!args.is_empty()).then_some(args)
}

/// Render a token back as selector source.
///
/// Differs from [`token_to_string`] in keeping the brackets and quotes, which
/// only matter when the text is going to be parsed as a selector again.
fn token_to_selector_text(token: &Token<'_>) -> String {
    match token {
        Token::SquareBracketBlock => "[".to_string(),
        Token::CloseSquareBracket => "]".to_string(),
        Token::ParenthesisBlock => "(".to_string(),
        Token::CloseParenthesis => ")".to_string(),
        Token::QuotedString(s) => format!("\"{}\"", cow_to_string(s)),
        other => token_to_string(other),
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
    // `:not(.a, .b)` is one selector, not two. Splitting on every comma tore
    // the tail of the argument off into a selector of its own.
    let mut depth = 0usize;

    for token in tokens.iter() {
        match token {
            Token::ParenthesisBlock | Token::Function(_) | Token::SquareBracketBlock => {
                depth += 1;
                current_tokens.push(token);
            }
            Token::CloseParenthesis | Token::CloseSquareBracket => {
                depth = depth.saturating_sub(1);
                current_tokens.push(token);
            }
            Token::Comma if depth == 0 => {
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
            Some(SimpleSelector::PseudoClass {
                name: cow_to_string(name),
                arguments: collect_function_arguments(tokens, i),
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
                        return Some(SimpleSelector::PseudoClass {
                            name: cow_to_string(name),
                            arguments: collect_function_arguments(tokens, i),
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

    // ------ Media queries ------

    /// A page's print stylesheet strips its navigation and swaps in a serif
    /// face. Applying it to the screen is how ja.wikipedia.org came out in
    /// mincho with no search box.
    #[test]
    fn print_only_rules_do_not_apply_to_the_screen() {
        assert!(!evaluate_media_condition("print", 1280.0));
        assert!(!evaluate_media_condition(
            "print and (min-width: 100px)",
            1280.0
        ));
        assert!(evaluate_media_condition("not print", 1280.0));
    }

    #[test]
    fn screen_and_all_apply() {
        assert!(evaluate_media_condition("screen", 1280.0));
        assert!(evaluate_media_condition("all", 1280.0));
        assert!(evaluate_media_condition("only screen", 1280.0));
        assert!(evaluate_media_condition("", 1280.0));
    }

    /// MediaWiki writes its breakpoints as `calc(640px - 1px)`. Read as a bare
    /// number that fails to parse, the condition used to be dropped and the
    /// block applied at every width — which is why a 1280px window got the
    /// narrow-screen layout.
    #[test]
    fn a_breakpoint_written_as_calc_is_evaluated() {
        assert!(!evaluate_media_condition(
            "all and (max-width:calc(640px - 1px))",
            1280.0
        ));
        assert!(evaluate_media_condition(
            "all and (max-width:calc(640px - 1px))",
            500.0
        ));
        assert!(!evaluate_media_condition(
            "screen and (max-width:calc(1120px - 1px))",
            1280.0
        ));
        assert!(evaluate_media_condition(
            "screen and (min-width:calc(640px - 1px))",
            1280.0
        ));
    }

    #[test]
    fn plain_width_breakpoints_still_work() {
        assert!(evaluate_media_condition(
            "screen and (min-width:1120px)",
            1280.0
        ));
        assert!(!evaluate_media_condition(
            "screen and (min-width:1680px)",
            1280.0
        ));
        assert!(evaluate_media_condition("(max-width: 1399px)", 1280.0));
        assert!(!evaluate_media_condition("(max-width: 640px)", 1280.0));
    }

    #[test]
    fn both_ends_of_a_range_must_hold() {
        let range = "screen and (min-width:640px) and (max-width:1120px)";
        assert!(evaluate_media_condition(range, 800.0));
        assert!(!evaluate_media_condition(range, 1280.0));
        assert!(!evaluate_media_condition(range, 400.0));
    }

    #[test]
    fn a_query_list_matches_if_any_query_does() {
        assert!(evaluate_media_condition("print, screen", 1280.0));
        assert!(!evaluate_media_condition("print, speech", 1280.0));
    }

    /// We render a light page and do not honour a motion preference, so the
    /// dark and reduced-motion blocks are not ours. They used to match because
    /// the condition mentioned neither `min-width` nor `max-width`.
    #[test]
    fn preference_features_answer_for_how_we_actually_render() {
        assert!(!evaluate_media_condition(
            "screen and (prefers-color-scheme:dark)",
            1280.0
        ));
        assert!(evaluate_media_condition(
            "screen and (prefers-color-scheme:light)",
            1280.0
        ));
        assert!(!evaluate_media_condition(
            "(prefers-reduced-motion:reduce)",
            1280.0
        ));
    }

    /// An unknown feature makes its query false. Failing open is what let a
    /// narrow-screen block style a wide window.
    #[test]
    fn an_unreadable_feature_fails_closed() {
        assert!(!evaluate_media_condition("(min-resolution: 2dppx)", 1280.0));
        assert!(!evaluate_media_condition(
            "screen and (max-width: banana)",
            1280.0
        ));
        assert!(!evaluate_media_condition(
            "screen and (min-width: 100px) and (min-resolution: 2dppx)",
            1280.0
        ));
    }

    // ------ @supports ------

    #[test]
    fn supports_holds_for_a_property_the_cascade_applies() {
        assert!(evaluate_supports_condition("(display:grid)"));
        assert!(evaluate_supports_condition("(display: flex)"));
        assert!(evaluate_supports_condition("(position:sticky)"));
    }

    /// Saying yes to a property we ignore is the harmful answer: MediaWiki
    /// pairs its mask-image icons with a `background-image` fallback and picks
    /// between them on exactly this question.
    #[test]
    fn supports_admits_a_property_we_do_not_have() {
        assert!(!evaluate_supports_condition("(mask-image:none)"));
        assert!(!evaluate_supports_condition("(-webkit-mask-image:none)"));
        assert!(!evaluate_supports_condition("(overflow-wrap:anywhere)"));
        assert!(!evaluate_supports_condition("(word-break:break-word)"));
    }

    #[test]
    fn supports_reads_not_and_or_and_nesting() {
        assert!(evaluate_supports_condition(
            "not ((-webkit-mask-image:none) or (mask-image:none))"
        ));
        assert!(!evaluate_supports_condition(
            "((-webkit-mask-image:none) or (mask-image:none))"
        ));
        assert!(evaluate_supports_condition(
            "(display:grid) or (mask-image:none)"
        ));
        assert!(!evaluate_supports_condition(
            "(display:grid) and (mask-image:none)"
        ));
        assert!(evaluate_supports_condition(
            "(display:grid) and (display:flex)"
        ));
    }

    /// A property we know carrying arithmetic we cannot evaluate.
    #[test]
    fn supports_rejects_a_value_function_we_cannot_evaluate() {
        assert!(!evaluate_supports_condition("(width:round(1.5px,1px))"));
        assert!(evaluate_supports_condition("(width:calc(100% - 1px))"));
    }

    /// `selector()` asks about our selector support, which we do not claim.
    #[test]
    fn supports_makes_no_claim_about_selectors() {
        assert!(!evaluate_supports_condition("selector(:focus-visible)"));
        assert!(evaluate_supports_condition("not selector(:focus-visible)"));
    }

    #[test]
    fn a_supported_block_contributes_its_rules() {
        let ss = parse_stylesheet("@supports (display:grid) { .a { color: red } }");
        assert_eq!(ss.rules.len(), 1, "the block's rules become ordinary rules");
    }

    #[test]
    fn an_unsupported_block_contributes_nothing() {
        let ss = parse_stylesheet("@supports (mask-image:none) { .a { color: red } }");
        assert_eq!(ss.rules.len(), 0);
    }

    #[test]
    fn a_media_rule_inside_supports_is_kept() {
        let ss = parse_stylesheet(
            "@supports (display:grid) { @media screen and (min-width:100px) { .a { color: red } } }",
        );
        assert_eq!(ss.media_rules.len(), 1);
        assert_eq!(ss.media_rules[0].rules.len(), 1);
    }

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

    // -- @keyframes parsing tests --

    #[test]
    fn keyframes_are_collected_with_their_name() {
        let ss = parse_stylesheet(
            "@keyframes slide { from { left: 0px } to { left: 100px } } div { color: red }",
        );
        assert_eq!(ss.keyframes.len(), 1);
        assert_eq!(ss.keyframes[0].name, "slide");
        assert_eq!(ss.keyframes[0].keyframes.len(), 2);
        assert_eq!(ss.rules.len(), 1, "the rule after it still parses");
    }

    #[test]
    fn a_vendor_prefixed_keyframes_rule_is_read_the_same_way() {
        let ss = parse_stylesheet("@-webkit-keyframes spin { from { left: 0 } to { left: 9px } }");
        assert_eq!(ss.keyframes.len(), 1);
        assert_eq!(ss.keyframes[0].name, "spin");
    }

    #[test]
    fn keyframes_inside_a_media_block_are_still_collected() {
        let ss = parse_stylesheet(
            "@media screen { @keyframes fade { from { color: red } to { color: blue } } }",
        );
        assert_eq!(ss.keyframes.len(), 1);
        assert_eq!(ss.keyframes[0].name, "fade");
    }

    #[test]
    fn an_empty_keyframes_block_is_dropped() {
        let ss = parse_stylesheet("@keyframes nothing { }");
        assert!(ss.keyframes.is_empty());
    }

    // -- @font-face parsing tests --

    #[test]
    fn font_face_reads_family_and_sources_in_order() {
        let ss = parse_stylesheet(
            r#"@font-face {
                 font-family: "Inter";
                 font-weight: 400;
                 src: url(/f/inter.woff2) format("woff2"),
                      url(/f/inter.woff) format("woff"),
                      url(/f/inter.ttf) format("truetype");
               }"#,
        );

        assert_eq!(ss.font_faces.len(), 1);
        let face = &ss.font_faces[0];
        assert_eq!(face.family, "Inter");
        assert_eq!(
            face.sources,
            vec![
                FontFaceSource::Url {
                    url: "/f/inter.woff2".into(),
                    format: Some("woff2".into())
                },
                FontFaceSource::Url {
                    url: "/f/inter.woff".into(),
                    format: Some("woff".into())
                },
                FontFaceSource::Url {
                    url: "/f/inter.ttf".into(),
                    format: Some("truetype".into())
                },
            ]
        );
    }

    #[test]
    fn font_face_reads_local_sources_and_bare_urls() {
        let ss = parse_stylesheet(
            "@font-face { font-family: Helvetica Neue; \
             src: local('Helvetica Neue'), url('h.otf'); }",
        );
        let face = &ss.font_faces[0];
        assert_eq!(face.family, "Helvetica Neue");
        assert_eq!(
            face.sources,
            vec![
                FontFaceSource::Local("Helvetica Neue".into()),
                FontFaceSource::Url {
                    url: "h.otf".into(),
                    format: None
                },
            ]
        );
    }

    #[test]
    fn a_font_face_without_a_source_is_dropped() {
        let ss = parse_stylesheet("@font-face { font-family: Ghost; font-weight: bold; }");
        assert!(ss.font_faces.is_empty());
    }

    #[test]
    fn font_face_does_not_disturb_the_rules_around_it() {
        let ss = parse_stylesheet(
            "a { color: red; } \
             @font-face { font-family: X; src: url(x.ttf); } \
             b { color: blue; }",
        );
        assert_eq!(ss.rules.len(), 2, "both style rules survive");
        assert_eq!(ss.font_faces.len(), 1);
    }

    #[test]
    fn font_face_inside_a_media_block_is_still_collected() {
        let ss =
            parse_stylesheet("@media screen { @font-face { font-family: M; src: url(m.ttf); } }");
        assert_eq!(ss.font_faces.len(), 1);
        assert_eq!(ss.font_faces[0].family, "M");
    }

    #[test]
    fn commas_inside_a_source_do_not_split_the_list() {
        // `local()` names may contain commas; splitting naively would produce
        // two broken sources instead of one.
        let sources = parse_font_face_src("local(\"Foo, Bar\"), url(a.ttf)");
        assert_eq!(
            sources,
            vec![
                FontFaceSource::Local("Foo, Bar".into()),
                FontFaceSource::Url {
                    url: "a.ttf".into(),
                    format: None
                },
            ]
        );
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
