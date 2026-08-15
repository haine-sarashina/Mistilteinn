pub mod parser;

use rustc_hash::FxHashMap;

// ------ Color ------

/// A CSS color value.
#[derive(Debug, Clone, PartialEq)]
pub enum CSSColor {
    /// #RRGGBB or #RGB
    Hex { r: u8, g: u8, b: u8 },
    /// rgba(r, g, b, a)
    Rgba { r: u8, g: u8, b: u8, a: f32 },
    /// Named color
    Named(String),
}

impl CSSColor {
    /// Convert to RGBA tuple (alpha in 0..=255).
    pub fn to_rgba(&self) -> (u8, u8, u8, u8) {
        match self {
            CSSColor::Hex { r, g, b } => (*r, *g, *b, 255),
            CSSColor::Rgba { r, g, b, a } => (*r, *g, *b, (*a * 255.0) as u8),
            CSSColor::Named(name) => {
                if let Some((r, g, b)) = parse_named_color(name) {
                    (r, g, b, 255)
                } else {
                    (0, 0, 0, 255)
                }
            }
        }
    }
}

/// Parses a CSS color value.
pub fn parse_color_value(color_str: &str) -> Option<CSSColor> {
    let color = color_str.trim();
    let lower = color.to_lowercase();

    // Hex colors: #RGB or #RRGGBB
    if color.starts_with('#') {
        let hex = &color[1..];
        match hex.len() {
            3 => {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..1].repeat(2), 16),
                    u8::from_str_radix(&hex[1..2].repeat(2), 16),
                    u8::from_str_radix(&hex[2..3].repeat(2), 16),
                ) {
                    return Some(CSSColor::Hex { r, g, b });
                }
            }
            6 => {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                ) {
                    return Some(CSSColor::Hex { r, g, b });
                }
            }
            _ => {}
        }
    }

    // rgb(r, g, b) and rgba(r, g, b, a) function notation
    if let Some(result) = parse_rgb_function(&lower) {
        return Some(result);
    }

    // Transparent keyword
    if lower == "transparent" {
        return Some(CSSColor::Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 0.0,
        });
    }

    // Named colors
    let named = parse_named_color(&lower);
    if let Some(_rgb) = named {
        return Some(CSSColor::Named(lower));
    }

    None
}

/// Parses `rgb(r, g, b)` or `rgba(r, g, b, a)` function notation.
/// Supports integer values (0-255) for R/G/B and float (0.0-1.0) for alpha.
/// Also handles percent values for R/G/B (0%-100% maps to 0-255).
fn parse_rgb_function(color: &str) -> Option<CSSColor> {
    let (is_rgba, inner) = if let Some(inner) = color
        .strip_prefix("rgba(")
        .and_then(|s| s.strip_suffix(')'))
    {
        (true, inner)
    } else if let Some(inner) = color.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        (false, inner)
    } else {
        return None;
    };

    let parts: Vec<&str> = inner.split(',').collect();
    let expected_count = if is_rgba { 4 } else { 3 };
    if parts.len() != expected_count {
        return None;
    }

    // Parse R, G, B components (integer 0-255 or percentage)
    let parse_component = |s: &str| -> Option<u8> {
        let val = s.trim();
        if val.ends_with('%') {
            // Percentage: 0%-100% maps to 0-255
            if let Ok(pct) = val[..val.len() - 1].trim().parse::<f32>() {
                return Some((pct / 100.0 * 255.0).clamp(0.0, 255.0) as u8);
            }
        } else {
            // Integer: 0-255
            if let Ok(n) = val.parse::<i32>() {
                return Some(n.clamp(0, 255) as u8);
            }
        }
        None
    };

    let r = parse_component(parts[0])?;
    let g = parse_component(parts[1])?;
    let b = parse_component(parts[2])?;

    if is_rgba {
        // Parse alpha: float 0.0-1.0
        let alpha_str = parts[3].trim();
        let alpha: f32 = alpha_str.parse().ok()?;
        if !(0.0..=1.0).contains(&alpha) {
            return None;
        }
        Some(CSSColor::Rgba { r, g, b, a: alpha })
    } else {
        // rgb() without alpha defaults to fully opaque
        Some(CSSColor::Rgba { r, g, b, a: 1.0 })
    }
}

fn parse_named_color(color: &str) -> Option<(u8, u8, u8)> {
    use std::collections::HashMap;
    let named_colors: HashMap<&str, (u8, u8, u8)> = [
        ("red", (255, 0, 0)),
        ("green", (0, 128, 0)),
        ("blue", (0, 0, 255)),
        ("white", (255, 255, 255)),
        ("black", (0, 0, 0)),
        ("yellow", (255, 255, 0)),
        ("cyan", (0, 255, 255)),
        ("magenta", (255, 0, 255)),
        ("orange", (255, 165, 0)),
        ("purple", (128, 0, 128)),
        ("pink", (255, 192, 203)),
        ("gray", (128, 128, 128)),
        ("grey", (128, 128, 128)),
        ("silver", (192, 192, 192)),
        ("maroon", (128, 0, 0)),
        ("olive", (128, 128, 0)),
        ("lime", (0, 255, 0)),
        ("aqua", (0, 255, 255)),
        ("teal", (0, 128, 128)),
        ("navy", (0, 0, 128)),
        ("fuchsia", (255, 0, 255)),
    ]
    .into_iter()
    .collect();

    named_colors.get(color).copied()
}

// ------ Declaration ------

/// A parsed CSS declaration (property: value).
#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
}

/// Parses a simple CSS declaration list into property-value pairs.
///
/// Example: `"color: red; margin: 10px"` → `[(color, red), (margin, 10px)]`
pub fn parse_declarations(source: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();

    for block in source.split(';') {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let parts: Vec<&str> = block.splitn(2, ':').map(|s| s.trim()).collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            let mut value = parts[1].to_string();
            let mut important = false;

            // Check for !important
            if let Some(idx) = value.find("!important") {
                important = true;
                value = value[..idx].trim().to_string();
            }

            if !value.is_empty() {
                declarations.push(Declaration {
                    property: parts[0].to_string(),
                    value,
                    important,
                });
            }
        }
    }

    declarations
}

/// Computes styles for a node (placeholder — cascade implementation TBD).
#[deprecated(since = "0.1.0", note = "Use compute_styles_for_tree instead")]
pub fn compute_styles(_properties: FxHashMap<String, String>) -> FxHashMap<String, String> {
    FxHashMap::default()
}

// ------ Cascade & Inheritance ------

/// CSS properties that inherit from the parent element.
///
/// Per CSS spec: most typography-related properties inherit,
/// but most box-model properties do not.
const INHERITABLE_PROPERTIES: &[&str] = &[
    "color",
    "font-size",
    "font-family",
    "font-weight",
    "font-style",
    "font-variant",
    "font",
    "letter-spacing",
    "line-height",
    "list-style-type",
    "list-style-position",
    "list-style-image",
    "list-style",
    "text-align",
    "text-decoration-color",
    "text-indent",
    "text-transform",
    "word-spacing",
    "direction",
    "visibility",
    "cursor",
];

/// Check if a CSS property name is inheritable.
fn is_inheritable(property: &str) -> bool {
    let lower = property.to_lowercase();
    INHERITABLE_PROPERTIES.iter().any(|&p| p == lower)
}

/// Clone only the inheritable properties from `parent` into `child`.
/// Non-inheritable properties use CSS initial (default) values.
fn inherit_properties(parent: &ComputedValues, mut child: ComputedValues) -> ComputedValues {
    // `color` inherits
    if parent.color.is_some() && child.color.is_none() {
        child.color = parent.color;
    }
    // `font-size` inherits
    if child.font_size == 16.0 && parent.font_size != 16.0 {
        child.font_size = parent.font_size;
    }
    // `font-family` inherits
    if child.font_family.is_empty() && !parent.font_family.is_empty() {
        child.font_family = parent.font_family.clone();
    }
    // `line-height` inherits
    if child.line_height == 1.2 && parent.line_height != 1.2 {
        child.line_height = parent.line_height;
    }
    // `text-align` inherits
    if child.text_align == TextAlign::Left && parent.text_align != TextAlign::Left {
        child.text_align = parent.text_align;
    }
    // `font-weight` / `font-style` inherit. Decoration lines do not technically
    // inherit, but they are drawn across descendant text, so propagating them
    // here produces the same result.
    // `letter-spacing`, `word-spacing` and `text-transform` inherit too, and
    // `merged_with` already resolves "child's own value wins" for each.
    child.text_style = child.text_style.merged_with(parent.text_style);
    // `visibility` inherits, but a child may declare `visible` to escape a
    // hidden ancestor, so only fill in when the child left it at the initial.
    if child.visibility == Visibility::Visible && parent.visibility != Visibility::Visible {
        child.visibility = parent.visibility;
    }
    // CSS custom properties (variables) always inherit
    for (k, v) in &parent.custom_properties {
        if !child.custom_properties.contains_key(k) {
            child.custom_properties.insert(k.clone(), v.clone());
        }
    }
    // `background_color` does NOT inherit (CSS initial = transparent)
    child
}

/// Look up a node's CSS `id` attribute.
fn node_get_id(node: &crate::html::DomNode) -> Option<&str> {
    node.get_attr("id")
}

/// Check if a node has a given CSS class.
fn node_has_class(node: &crate::html::DomNode, class: &str) -> bool {
    node.get_attr("class")
        .map(|c| c.split_whitespace().any(|cls| cls == class))
        .unwrap_or(false)
}

/// Resolve the font size of the root `<html>` element, which `rem` units
/// resolve against.
///
/// Only selectors that target the root itself (`html`, `:root`, `*`) can apply,
/// so this is a cheap scan rather than a full cascade pass. Declarations are
/// applied in ascending specificity, each `em` resolving against the value so
/// far — matching how `font-size: 1.2em` on the root behaves in a real engine.
fn resolve_root_font_size(
    arena: &crate::html::DomArena,
    active_rules: &[&parser::CSSRule],
    ctx: LengthContext,
) -> f32 {
    // Inline `style` on <html> wins over any stylesheet rule, so find the node.
    let root_style_attr = {
        let nodes = arena.nodes.borrow();
        nodes
            .iter()
            .find(|n| {
                n.tag_name()
                    .is_some_and(|t| t.as_ref().eq_ignore_ascii_case("html"))
            })
            .and_then(|n| n.get_attr("style").map(|s| s.to_string()))
    };

    let mut candidates: Vec<(&Declaration, (u32, u32, u32))> = Vec::new();
    for rule in active_rules {
        let targets_root = rule.selectors.iter().any(|sel| {
            sel.complex.len() == 1
                && match &sel.complex[0].1 {
                    parser::SimpleSelector::Type(t) => t.eq_ignore_ascii_case("html"),
                    parser::SimpleSelector::PseudoClass { name, .. } => {
                        name.eq_ignore_ascii_case("root")
                    }
                    parser::SimpleSelector::Universal => true,
                    _ => false,
                }
        });
        if !targets_root {
            continue;
        }
        let spec = rule
            .selectors
            .iter()
            .map(|s| s.specificity())
            .max()
            .unwrap_or((0, 0, 0));
        for decl in &rule.declarations {
            if decl.property.eq_ignore_ascii_case("font-size") {
                candidates.push((decl, spec));
            }
        }
    }

    // Normal declarations first, then !important, each in ascending specificity.
    candidates.sort_by_key(|(decl, spec)| (decl.important, *spec));

    let mut size = DEFAULT_FONT_SIZE;
    let mut apply = |value: &str, size: &mut f32| {
        let local = LengthContext {
            font_size: *size,
            root_font_size: *size,
            ..ctx
        };
        // A percentage on the root resolves against the initial font size.
        if let Some(pct) = value.trim().strip_suffix('%') {
            if let Ok(n) = pct.trim().parse::<f32>() {
                *size = DEFAULT_FONT_SIZE * n / 100.0;
                return;
            }
        }
        if let Some(px) = parse_length_ctx(value, local) {
            if px > 0.0 {
                *size = px;
            }
        }
    };

    for (decl, _) in candidates {
        apply(&decl.value, &mut size);
    }
    if let Some(style) = root_style_attr {
        for decl in parse_declarations(&style) {
            if decl.property.eq_ignore_ascii_case("font-size") {
                apply(&decl.value, &mut size);
            }
        }
    }

    size
}

/// Compute the resolved styles for every element node in the DOM tree.
///
/// Takes the parsed DOM arena and a `Stylesheet` (from `parse_css`),
/// applies the CSS cascade (specificity → source order → `!important`),
/// then propagates inheritable properties from parent to child.
///
/// Returns a map of `NodeId(u32)` → `ComputedValues` for every element node.
pub fn compute_styles_for_tree(
    arena: &crate::html::DomArena,
    stylesheet: &parser::Stylesheet,
    viewport: (f32, f32),
) -> FxHashMap<u32, ComputedValues> {
    compute_styles_for_tree_internal(arena, stylesheet, viewport, |_| false)
}

/// Compute styles with runtime hover context.
///
/// `hovered_ids` is the set of DOM node IDs currently under the mouse cursor,
/// including ancestors (for selectors like `div:hover > a`).
pub fn compute_styles_for_tree_with_hover(
    arena: &crate::html::DomArena,
    stylesheet: &parser::Stylesheet,
    viewport: (f32, f32),
    hovered_ids: &[u32],
) -> FxHashMap<u32, ComputedValues> {
    let hover_set: std::collections::HashSet<u32> = hovered_ids.iter().copied().collect();
    compute_styles_for_tree_internal(arena, stylesheet, viewport, |id| hover_set.contains(&id))
}

/// Internal implementation of style computation with a configurable hover predicate.
///
/// The `is_hovered` closure returns `true` for elements that should match `:hover`.
/// Pass `|_id| false` for static (no-hover) computation.
fn compute_styles_for_tree_internal<F>(
    arena: &crate::html::DomArena,
    stylesheet: &parser::Stylesheet,
    viewport: (f32, f32),
    is_hovered: F,
) -> FxHashMap<u32, ComputedValues>
where
    F: Fn(u32) -> bool,
{
    let mut result = FxHashMap::default();

    // Collect all element node IDs by iterating the arena in index order.
    // html5ever assigns IDs in document order (0 = document, then children).
    let nodes_ref = arena.nodes.borrow();
    let element_ids: Vec<u32> = nodes_ref
        .iter()
        .enumerate()
        .filter_map(|(idx, node)| {
            if node.is_element() {
                Some(idx as u32)
            } else {
                None
            }
        })
        .collect();

    // Pre-compute child relationships for pseudo-classes (:first-child, :last-child, :only-child, :nth-child)
    let mut first_children: std::collections::HashSet<u32> =
        std::collections::HashSet::with_capacity(element_ids.len());
    let mut last_children: std::collections::HashSet<u32> =
        std::collections::HashSet::with_capacity(element_ids.len());
    let mut only_children: std::collections::HashSet<u32> =
        std::collections::HashSet::with_capacity(element_ids.len());
    let mut child_indices: rustc_hash::FxHashMap<u32, usize> = rustc_hash::FxHashMap::default();

    let mut node_classes: rustc_hash::FxHashMap<u32, Vec<String>> =
        rustc_hash::FxHashMap::default();
    let mut node_ids: rustc_hash::FxHashMap<u32, String> = rustc_hash::FxHashMap::default();

    for node in &*nodes_ref {
        let elem_children: Vec<u32> = node
            .children
            .iter()
            .map(|h| h.index() as u32)
            .filter(|&id| element_ids.contains(&id))
            .collect();

        if let Some(&first_child) = elem_children.first() {
            first_children.insert(first_child);
        }
        if let Some(&last_child) = elem_children.last() {
            last_children.insert(last_child);
        }
        if elem_children.len() == 1 {
            only_children.insert(elem_children[0]);
        }
        for (idx, &cid) in elem_children.iter().enumerate() {
            child_indices.insert(cid, idx + 1); // 1-based index
        }
    }

    for &node_id in &element_ids {
        let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(node_id));
        if let Some(node) = arena.get(handle) {
            if let Some(attr) = node.get_attr("class") {
                node_classes.insert(node_id, attr.split_whitespace().map(String::from).collect());
            }
            if let Some(id) = node.get_attr("id") {
                node_ids.insert(node_id, id.to_string());
            }
        }
    }

    drop(nodes_ref);

    // Phase 1: Cascade — for each element, find matching rules and apply
    let start = std::time::Instant::now();
    let total_elements = element_ids.len();

    // Collect active rules from the stylesheet based on viewport width
    let mut active_rules: Vec<&parser::CSSRule> = stylesheet.rules.iter().collect();
    for media_rule in &stylesheet.media_rules {
        if parser::evaluate_media_condition(&media_rule.condition, viewport.0) {
            active_rules.extend(media_rule.rules.iter());
        }
    }

    // Keep applied declarations per element so we can re-evaluate variables after inheriting from parent
    let mut applied_decls_per_element: rustc_hash::FxHashMap<u32, Vec<Declaration>> =
        rustc_hash::FxHashMap::default();

    // Base context for resolving relative lengths. `rem` needs the root font
    // size, which is whatever `<html>` resolves to, so compute that first.
    let mut length_ctx = LengthContext {
        font_size: DEFAULT_FONT_SIZE,
        root_font_size: DEFAULT_FONT_SIZE,
        viewport_width: viewport.0,
        viewport_height: viewport.1,
    };
    length_ctx.root_font_size = resolve_root_font_size(arena, &active_rules, length_ctx);

    for (i, &node_id) in element_ids.iter().enumerate() {
        if i % 1000 == 0 && i > 0 {
            log::info!("Computed styles for {}/{} elements...", i, total_elements);
        }
        let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(node_id));
        let node = match arena.get(handle) {
            Some(n) => n,
            None => continue,
        };

        let get_parent = |id: u32| -> Option<u32> {
            let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
            if let Some(node) = arena.get(handle) {
                node.parent_id()
            } else {
                None
            }
        };

        let simple_match = |id: u32, sel: &parser::SimpleSelector| -> bool {
            let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
            let node = match arena.get(handle) {
                Some(n) => n,
                None => return false,
            };
            let tag = match node.tag_name() {
                Some(t) => t,
                None => return false,
            };
            let has_class = |c: &str| -> bool {
                node_classes
                    .get(&id)
                    .map_or(false, |classes| classes.iter().any(|cls| cls == c))
            };
            let has_id_val =
                |i: &str| -> bool { node_ids.get(&id).map_or(false, |id_str| id_str == i) };
            let matches_attr = |name: &str, op: &parser::AttrOperator, val: Option<&str>| -> bool {
                if let Some(attr_val) = node.get_attr(name) {
                    parser::evaluate_attr_operator(attr_val, op, val)
                } else {
                    false
                }
            };
            let is_first = || first_children.contains(&id);
            let is_last = || last_children.contains(&id);
            let is_only = || only_children.contains(&id);
            let child_idx = || *child_indices.get(&id).unwrap_or(&1);
            let hover = || is_hovered(id);
            parser::Selector::simple_matches_with_context(
                sel,
                tag,
                has_class,
                has_id_val,
                matches_attr,
                is_first,
                is_last,
                is_only,
                child_idx,
                hover,
            )
        };

        // Collect all matching rules with their specificity
        let mut matched: Vec<(&parser::CSSRule, (u32, u32, u32))> = Vec::new();

        for rule in &active_rules {
            for selector in &rule.selectors {
                if selector.full_matches(node_id, &get_parent, &simple_match) {
                    matched.push((rule, selector.specificity()));
                }
            }
        }

        let mut computed = ComputedValues::default();

        // Collect !important and normal declarations
        let mut important_decls: Vec<(&Declaration, (u32, u32, u32))> = Vec::new();
        let mut normal_decls: Vec<(&Declaration, (u32, u32, u32))> = Vec::new();
        for (rule, spec) in &matched {
            for decl in &rule.declarations {
                if decl.important {
                    important_decls.push((decl, *spec));
                } else {
                    normal_decls.push((decl, *spec));
                }
            }
        }

        let mut all_applied = Vec::new();

        // Pass 1: Normal declarations (ascending specificity — higher wins by overwriting)
        normal_decls.sort_by(|a, b| a.1.cmp(&b.1));
        for (decl, _) in normal_decls {
            computed = computed.from_declaration_with_ctx(decl, length_ctx);
            all_applied.push((*decl).clone());
        }

        // Pass 2: !important declarations (override normal, ascending specificity)
        important_decls.sort_by(|a, b| a.1.cmp(&b.1));
        for (decl, _) in important_decls {
            computed = computed.from_declaration_with_ctx(decl, length_ctx);
            all_applied.push((*decl).clone());
        }

        // Pass 3: Inline `style` attribute — highest specificity per CSS spec.
        if let Some(style_attr) = node.get_attr("style") {
            let inline_decls = parse_declarations(style_attr);
            for decl in &inline_decls {
                computed = computed.from_declaration_with_ctx(decl, length_ctx);
                all_applied.push(decl.clone());
            }
        }

        applied_decls_per_element.insert(node_id, all_applied);
        result.insert(node_id, computed);
    }

    log::info!("Phase 1 Cascade complete in {:?}", start.elapsed());

    // Phase 2: Inheritance & Defaults — top-down pass
    // Build parent map using the public parent_id() accessor
    let mut parent_map = FxHashMap::default();
    for &nid in &element_ids {
        let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(nid));
        if let Some(node) = arena.get(handle) {
            if let Some(pid) = node.parent_id() {
                parent_map.insert(nid, pid);
            }
        }
    }

    // Topological sort: process nodes in document order
    for &node_id in &element_ids {
        if let Some(&parent_id) = parent_map.get(&node_id) {
            if let Some(parent_styles) = result.get(&parent_id).cloned() {
                // Rebuild from a freshly inherited base rather than layering onto
                // the Phase 1 result. Every declaration is re-applied below, so
                // reusing the Phase 1 values would apply them twice — harmless for
                // absolute values, but `em` would compound (2em → 4x the parent).
                let mut inherited = inherit_properties(&parent_styles, ComputedValues::default());

                // Re-evaluate applied declarations with inherited CSS custom properties
                if let Some(decls) = applied_decls_per_element.get(&node_id) {
                    for decl in decls {
                        inherited = inherited.from_declaration_with_ctx(decl, length_ctx);
                    }
                }

                result.insert(node_id, inherited);
            }
        }
    }

    result
}

// ------ Display Type ------

/// The computed `display` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayType {
    Block,
    Inline,
    InlineBlock,
    Flex,
    InlineFlex,
    None,
    Grid,
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableCaption,
    TableColumn,
    TableColumnGroup,
}

// ------ Flexbox Enums ------

/// The computed `flex-direction` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

/// The computed `flex-wrap` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

/// The computed `justify-content` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JustifyContent {
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
}

/// The computed `align-items` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignItems {
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
}

/// The computed `align-content` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignContent {
    Normal,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    Stretch,
}

// ------ Overflow ------

/// The computed `overflow` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
    Auto,
}

// ------ Box Sizing ------

/// The computed `box-sizing` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

// ------ Flex Basis ------

/// The computed `flex-basis` CSS property.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexBasis {
    Auto,
    Pixels(f32),
    Percentage(f32), // stored as fraction (0.5 for 50%)
}

// ------ Grid Track Sizing ------

/// The size of a single grid track (column or row).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridTrack {
    /// Fixed pixel width/height, e.g. "150px"
    Fixed(f32),
    /// Fractional unit, e.g. "1fr", "2fr"
    Fr(f32),
    /// Automatic sizing, i.e. "auto"
    Auto,
    /// Minimum content sizing, i.e. "min-content"
    MinContent,
    /// Maximum content sizing, i.e. "max-content"
    MaxContent,
    /// Fit content with a maximum limit, i.e. "fit-content(200px)"
    FitContent(f32),
}

// ------ Alignment on Item (align-self) ------

/// The computed `align-self` CSS property for flex/grid items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignSelf {
    Auto,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Stretch,
}

// ------ Positioning ------

/// The computed `position` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionType {
    Static,
    Relative,
    Absolute,
}

// ------ Float & Clear ------

/// The computed `float` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatType {
    None,
    Left,
    Right,
}

/// The computed `clear` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearType {
    None,
    Left,
    Right,
    Both,
}

// ------ Computed Values ------

/// Fully resolved CSS property values for a single element.
#[derive(Debug, Clone)]
pub struct ComputedValues {
    pub display: DisplayType,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Margin: [top, right, bottom, left]
    pub margin: [f32; 4],
    /// Margin auto flags: [top, right, bottom, left]
    pub margin_auto: [bool; 4],
    /// Padding: [top, right, bottom, left]
    pub padding: [f32; 4],
    /// Background color as RGBA (None = transparent)
    pub background_color: Option<[u8; 4]>,
    /// Text color as RGBA (None = not set)
    pub color: Option<[u8; 4]>,
    pub font_size: f32,
    pub font_family: String,
    /// Flex container properties
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub align_content: AlignContent,
    /// Gap between flex lines (row_gap = space between rows, column_gap = space between columns)
    pub row_gap: f32,
    pub column_gap: f32,
    /// Flex item properties
    pub flex_grow: f32,
    pub flex_shrink: f32,
    /// Flex basis
    pub flex_basis: FlexBasis,
    /// Box sizing model
    pub box_sizing: BoxSizing,
    /// Explicit width/height from CSS (for border-box support)
    pub explicit_width: Option<f32>,
    pub explicit_height: Option<f32>,
    /// Min/max width constraints
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    /// Normalized line-height multiplier (CSS default = 1.2 for "normal").
    pub line_height: f32,
    // Overflow
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    // Positioning
    pub position: PositionType,
    pub offset_top: Option<f32>,
    pub offset_right: Option<f32>,
    pub offset_bottom: Option<f32>,
    pub offset_left: Option<f32>,
    // Grid container properties
    pub grid_template_columns: Vec<GridTrack>,
    pub grid_template_rows: Vec<GridTrack>,
    pub grid_column_gap: f32,
    pub grid_row_gap: f32,
    // Grid item / flex item property
    pub order: i32,
    pub align_self: AlignSelf,
    // Float & Clear
    pub float: FloatType,
    pub clear: ClearType,
    // Border & Radius
    pub border_width: [f32; 4],
    pub border_color: Option<[u8; 4]>,
    pub border_radius: f32,
    // Typography
    pub text_style: TextStyleFlags,
    pub text_align: TextAlign,
    pub visibility: Visibility,
    // CSS Custom Properties (CSS variables --*)
    pub custom_properties: rustc_hash::FxHashMap<String, String>,
}

/// How `text-transform` rewrites a run's text before it is measured and drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

impl TextTransform {
    /// Apply the transform, returning the original string when it is a no-op.
    pub fn apply(self, text: &str) -> std::borrow::Cow<'_, str> {
        use std::borrow::Cow;
        match self {
            Self::None => Cow::Borrowed(text),
            Self::Uppercase => Cow::Owned(text.to_uppercase()),
            Self::Lowercase => Cow::Owned(text.to_lowercase()),
            Self::Capitalize => {
                // Uppercase the first letter of each whitespace-separated word,
                // leaving the rest of the word as authored (per CSS).
                let mut out = String::with_capacity(text.len());
                let mut at_word_start = true;
                for ch in text.chars() {
                    if ch.is_whitespace() {
                        at_word_start = true;
                        out.push(ch);
                    } else if at_word_start {
                        at_word_start = false;
                        out.extend(ch.to_uppercase());
                    } else {
                        out.push(ch);
                    }
                }
                Cow::Owned(out)
            }
        }
    }
}

/// The computed `visibility` property.
///
/// Hidden and collapsed elements still take part in layout — they are simply
/// not painted — which is what separates this from `display: none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

impl Visibility {
    /// Whether an element with this visibility should be painted.
    pub fn is_painted(self) -> bool {
        matches!(self, Self::Visible)
    }
}

/// The text styling that travels from the cascade through layout into the
/// glyph rasterizer.
///
/// Kept as one `Copy` struct rather than loose fields because every text box,
/// inline run, and render command has to carry the whole set.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextStyleFlags {
    /// `font-weight: bold` (or numeric >= 600).
    pub bold: bool,
    /// `font-style: italic | oblique`.
    pub italic: bool,
    /// `text-decoration-line: underline`.
    pub underline: bool,
    /// `text-decoration-line: line-through`.
    pub line_through: bool,
    /// `text-decoration-line: overline`.
    pub overline: bool,
    /// `letter-spacing` in pixels; 0 is the initial `normal`.
    pub letter_spacing: f32,
    /// `word-spacing` in pixels; 0 is the initial `normal`.
    pub word_spacing: f32,
    /// `text-transform`.
    pub transform: TextTransform,
}

impl TextStyleFlags {
    /// Whether any decoration line needs to be drawn.
    pub fn has_decoration(&self) -> bool {
        self.underline || self.line_through || self.overline
    }

    /// Combine with styles coming from an ancestor.
    ///
    /// A flag set anywhere up the chain stays set: an `<em>` inside a bold
    /// heading is both bold and italic, and a decoration on an ancestor is
    /// drawn across the descendant text it contains. Spacing and transform are
    /// single-valued, so the element's own value wins when it set one.
    pub fn merged_with(self, inherited: Self) -> Self {
        Self {
            bold: self.bold || inherited.bold,
            italic: self.italic || inherited.italic,
            underline: self.underline || inherited.underline,
            line_through: self.line_through || inherited.line_through,
            overline: self.overline || inherited.overline,
            letter_spacing: if self.letter_spacing != 0.0 {
                self.letter_spacing
            } else {
                inherited.letter_spacing
            },
            word_spacing: if self.word_spacing != 0.0 {
                self.word_spacing
            } else {
                inherited.word_spacing
            },
            transform: if self.transform != TextTransform::None {
                self.transform
            } else {
                inherited.transform
            },
        }
    }
}

/// Decide whether a `font-weight` value renders as bold.
///
/// Accepts the keywords plus numeric weights, treating 600 and above as bold
/// (the threshold browsers use when only a regular and a bold face exist).
fn parse_is_bold(val: &str) -> bool {
    let val = val.trim();
    if let Ok(n) = val.parse::<f32>() {
        return n >= 600.0;
    }
    matches!(val.to_ascii_lowercase().as_str(), "bold" | "bolder")
}

/// CSS text-align property
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Default for ComputedValues {
    /// CSS initial values per the CSS specification.
    fn default() -> Self {
        Self {
            display: DisplayType::Inline,
            width: None,
            height: None,
            margin: [0.0; 4],
            margin_auto: [false; 4],
            padding: [0.0; 4],
            background_color: None,
            color: None,
            font_size: 16.0,
            font_family: String::new(),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            align_content: AlignContent::Normal,
            align_self: AlignSelf::Auto,
            row_gap: 0.0,
            column_gap: 0.0,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: FlexBasis::Auto,
            box_sizing: BoxSizing::ContentBox,
            explicit_width: None,
            explicit_height: None,
            min_width: None,
            max_width: None,
            line_height: 1.2,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            position: PositionType::Static,
            offset_top: None,
            offset_right: None,
            offset_bottom: None,
            offset_left: None,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_column_gap: 0.0,
            grid_row_gap: 0.0,
            order: 0,
            float: FloatType::None,
            clear: ClearType::None,
            border_width: [0.0; 4],
            border_color: None,
            border_radius: 0.0,
            text_style: TextStyleFlags::default(),
            text_align: TextAlign::Left,
            visibility: Visibility::Visible,
            custom_properties: rustc_hash::FxHashMap::default(),
        }
    }
}

/// Resolve `var(--name, fallback)` occurrences within a CSS property value.
pub fn resolve_var_functions(val: &str, vars: &rustc_hash::FxHashMap<String, String>) -> String {
    let mut seen = Vec::new();
    resolve_var_functions_internal(val, vars, &mut seen, 0)
}

fn resolve_var_functions_internal(
    val: &str,
    vars: &rustc_hash::FxHashMap<String, String>,
    seen: &mut Vec<String>,
    depth: usize,
) -> String {
    if depth > 10 || !val.contains("var(") {
        return val.to_string();
    }

    let mut result = String::new();
    let mut i = 0;
    let bytes = val.as_bytes();

    while i < val.len() {
        if val[i..].starts_with("var(") {
            let start = i + 4;
            let mut paren_depth = 1usize;
            let mut end = start;
            while end < bytes.len() && paren_depth > 0 {
                if bytes[end] == b'(' {
                    paren_depth += 1;
                } else if bytes[end] == b')' {
                    paren_depth -= 1;
                }
                end += 1;
            }
            if paren_depth == 0 {
                let inside = val[start..end - 1].trim();
                let (var_name, fallback) = if let Some(comma_pos) = find_top_level_comma(inside) {
                    (
                        inside[..comma_pos].trim(),
                        Some(inside[comma_pos + 1..].trim()),
                    )
                } else {
                    (inside, None)
                };

                let resolved_value = if !seen.iter().any(|s| s == var_name) {
                    if let Some(found) = vars.get(var_name) {
                        seen.push(var_name.to_string());
                        let res = resolve_var_functions_internal(found, vars, seen, depth + 1);
                        seen.pop();
                        res
                    } else if let Some(fb) = fallback {
                        resolve_var_functions_internal(fb, vars, seen, depth + 1)
                    } else {
                        String::new()
                    }
                } else if let Some(fb) = fallback {
                    resolve_var_functions_internal(fb, vars, seen, depth + 1)
                } else {
                    String::new()
                };

                result.push_str(&resolved_value);
                i = end;
                continue;
            }
        }

        let c = val[i..].chars().next().unwrap();
        result.push(c);
        i += c.len_utf8();
    }

    result
}

fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate() {
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            if depth > 0 {
                depth -= 1;
            }
        } else if b == b',' && depth == 0 {
            return Some(i);
        }
    }
    None
}

impl ComputedValues {
    /// Parse a single [`Declaration`] and apply it to a clone of `self`.
    ///
    /// Returns a new `ComputedValues` with the declaration applied on top
    /// of the current values (or defaults if called on `Default::default()`).
    pub fn from_declaration(self, decl: &Declaration) -> Self {
        self.from_declaration_with_ctx(decl, LengthContext::default())
    }

    /// Parse a single [`Declaration`] and apply it, resolving relative length
    /// units (`em`, `rem`, `vw`, `vh`, …) against `base_ctx`.
    ///
    /// `base_ctx.font_size` is ignored: `em` always resolves against the font
    /// size this element has accumulated so far, which — because inheritance
    /// runs before declarations are re-applied — is the parent's font size for
    /// a `font-size` declaration, and the element's own font size afterwards.
    pub fn from_declaration_with_ctx(
        mut self,
        decl: &Declaration,
        base_ctx: LengthContext,
    ) -> Self {
        let prop = decl.property.to_lowercase();
        if prop.starts_with("--") {
            self.custom_properties
                .insert(decl.property.clone(), decl.value.clone());
            return self;
        }

        let resolved_val = resolve_var_functions(&decl.value, &self.custom_properties);
        let val = resolved_val.trim();

        let ctx = LengthContext {
            font_size: self.font_size,
            ..base_ctx
        };
        // Shadow the module-level helpers so every length in this function
        // resolves through the active context.
        let parse_length = |s: &str| parse_length_ctx(s, ctx);
        let parse_box_four = |s: &str, fallback: [f32; 4]| parse_box_four(s, fallback, ctx);
        let parse_margin_box_four =
            |s: &str, fm: [f32; 4], fa: [bool; 4]| parse_margin_box_four(s, fm, fa, ctx);
        let parse_flex_basis = |s: &str| parse_flex_basis(s, ctx);
        let parse_grid_track_list = |s: &str| parse_grid_track_list(s, ctx);

        match prop.as_str() {
            "display" => {
                self.display = match val {
                    "block" => DisplayType::Block,
                    "inline" => DisplayType::Inline,
                    "inline-block" => DisplayType::InlineBlock,
                    "flex" => DisplayType::Flex,
                    "inline-flex" => DisplayType::InlineFlex,
                    "none" => DisplayType::None,
                    "grid" => DisplayType::Grid,
                    "table" => DisplayType::Table,
                    "inline-table" => DisplayType::InlineTable,
                    "table-row-group" => DisplayType::TableRowGroup,
                    "table-header-group" => DisplayType::TableHeaderGroup,
                    "table-footer-group" => DisplayType::TableFooterGroup,
                    "table-row" => DisplayType::TableRow,
                    "table-cell" => DisplayType::TableCell,
                    "table-caption" => DisplayType::TableCaption,
                    "table-column" => DisplayType::TableColumn,
                    "table-column-group" => DisplayType::TableColumnGroup,
                    _ => self.display,
                };
            }
            "width" => {
                self.width = parse_length(val);
                self.explicit_width = self.width;
            }
            "height" => {
                self.height = parse_length(val);
                self.explicit_height = self.height;
            }
            "margin" => {
                let (m, a) = parse_margin_box_four(val, self.margin, self.margin_auto);
                self.margin = m;
                self.margin_auto = a;
            }
            "margin-top" => {
                if val.trim().eq_ignore_ascii_case("auto") {
                    self.margin[0] = 0.0;
                    self.margin_auto[0] = true;
                } else if let Some(v) = parse_length(val) {
                    self.margin[0] = v;
                    self.margin_auto[0] = false;
                }
            }
            "margin-right" => {
                if val.trim().eq_ignore_ascii_case("auto") {
                    self.margin[1] = 0.0;
                    self.margin_auto[1] = true;
                } else if let Some(v) = parse_length(val) {
                    self.margin[1] = v;
                    self.margin_auto[1] = false;
                }
            }
            "margin-bottom" => {
                if val.trim().eq_ignore_ascii_case("auto") {
                    self.margin[2] = 0.0;
                    self.margin_auto[2] = true;
                } else if let Some(v) = parse_length(val) {
                    self.margin[2] = v;
                    self.margin_auto[2] = false;
                }
            }
            "margin-left" => {
                if val.trim().eq_ignore_ascii_case("auto") {
                    self.margin[3] = 0.0;
                    self.margin_auto[3] = true;
                } else if let Some(v) = parse_length(val) {
                    self.margin[3] = v;
                    self.margin_auto[3] = false;
                }
            }
            "padding" => {
                self.padding = parse_box_four(val, self.padding);
            }
            "padding-top" => {
                if let Some(v) = parse_length(val) {
                    self.padding[0] = v;
                }
            }
            "padding-right" => {
                if let Some(v) = parse_length(val) {
                    self.padding[1] = v;
                }
            }
            "padding-bottom" => {
                if let Some(v) = parse_length(val) {
                    self.padding[2] = v;
                }
            }
            "padding-left" => {
                if let Some(v) = parse_length(val) {
                    self.padding[3] = v;
                }
            }
            "background-color" | "background" => {
                if let Some(color) = parse_color_value(val) {
                    let (r, g, b, a) = color.to_rgba();
                    self.background_color = Some([r, g, b, a]);
                }
            }
            "color" => {
                if let Some(color) = parse_color_value(val) {
                    let (r, g, b, a) = color.to_rgba();
                    self.color = Some([r, g, b, a]);
                }
            }
            "font-size" => {
                if let Some(v) = parse_length(val) {
                    self.font_size = v;
                }
            }
            "font-family" => {
                // Strip quotes if present
                self.font_family = val.trim_matches(|c| c == '"' || c == '\'').to_string();
            }
            "flex-direction" => {
                self.flex_direction = match val {
                    "row" => FlexDirection::Row,
                    "column" => FlexDirection::Column,
                    "row-reverse" => FlexDirection::RowReverse,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    _ => self.flex_direction,
                };
            }
            "flex-wrap" => {
                self.flex_wrap = match val {
                    "nowrap" => FlexWrap::NoWrap,
                    "wrap" => FlexWrap::Wrap,
                    "wrap-reverse" => FlexWrap::WrapReverse,
                    _ => self.flex_wrap,
                };
            }
            "justify-content" => {
                self.justify_content = match val {
                    "flex-start" => JustifyContent::FlexStart,
                    "flex-end" => JustifyContent::FlexEnd,
                    "center" => JustifyContent::Center,
                    "space-between" => JustifyContent::SpaceBetween,
                    "space-around" => JustifyContent::SpaceAround,
                    _ => self.justify_content,
                };
            }
            "justify-items" => {
                if val.trim().eq_ignore_ascii_case("center") {
                    self.text_align = TextAlign::Center;
                }
            }
            "align-items" => {
                self.align_items = match val {
                    "stretch" => AlignItems::Stretch,
                    "flex-start" => AlignItems::FlexStart,
                    "flex-end" => AlignItems::FlexEnd,
                    "center" => AlignItems::Center,
                    _ => self.align_items,
                };
            }
            "flex-grow" => {
                if let Ok(v) = val.parse::<f32>() {
                    self.flex_grow = v;
                }
            }
            "flex-shrink" => {
                if let Ok(v) = val.parse::<f32>() {
                    self.flex_shrink = v;
                }
            }
            "flex-basis" => {
                self.flex_basis = parse_flex_basis(val);
            }
            "flex" => {
                parse_flex_shorthand(&mut self, val, ctx);
            }
            "align-content" => {
                self.align_content = parse_align_content(val);
            }
            "gap" => {
                let gaps: Vec<f32> = val
                    .split_whitespace()
                    .filter_map(|p| parse_length(p))
                    .collect();
                match gaps.len() {
                    1 => {
                        self.row_gap = gaps[0];
                        self.column_gap = gaps[0];
                    }
                    2 => {
                        self.row_gap = gaps[0];
                        self.column_gap = gaps[1];
                    }
                    _ => {}
                }
            }
            "row-gap" => {
                if let Some(v) = parse_length(val) {
                    self.row_gap = v;
                }
            }
            "column-gap" => {
                if let Some(v) = parse_length(val) {
                    self.column_gap = v;
                }
            }
            "line-height" => {
                if val.eq_ignore_ascii_case("normal") {
                    self.line_height = 1.2; // CSS default for normal
                } else if let Ok(v) = val.parse::<f32>() {
                    // Unitless number → normalized multiplier
                    self.line_height = v;
                } else if let Some(px) = parse_length(val) {
                    // Pixel value → convert to normalized multiplier relative to font-size
                    self.line_height = px / self.font_size;
                }
            }
            "overflow" => {
                let parsed = match val {
                    "hidden" => Overflow::Hidden,
                    "scroll" => Overflow::Scroll,
                    "auto" => Overflow::Auto,
                    _ => Overflow::Visible,
                };
                self.overflow_x = parsed;
                self.overflow_y = parsed;
            }
            "overflow-x" => {
                self.overflow_x = match val {
                    "hidden" => Overflow::Hidden,
                    "scroll" => Overflow::Scroll,
                    "auto" => Overflow::Auto,
                    _ => Overflow::Visible,
                };
            }
            "overflow-y" => {
                self.overflow_y = match val {
                    "hidden" => Overflow::Hidden,
                    "scroll" => Overflow::Scroll,
                    "auto" => Overflow::Auto,
                    _ => Overflow::Visible,
                };
            }
            "float" => {
                self.float = match val {
                    "left" => FloatType::Left,
                    "right" => FloatType::Right,
                    _ => FloatType::None,
                };
            }
            "clear" => {
                self.clear = match val {
                    "left" => ClearType::Left,
                    "right" => ClearType::Right,
                    "both" => ClearType::Both,
                    _ => ClearType::None,
                };
            }
            "position" => {
                self.position = match val {
                    "relative" => PositionType::Relative,
                    "absolute" => PositionType::Absolute,
                    _ => PositionType::Static,
                };
            }
            "top" => {
                self.offset_top = parse_offset(val);
            }
            "right" => {
                self.offset_right = parse_offset(val);
            }
            "bottom" => {
                self.offset_bottom = parse_offset(val);
            }
            "left" => {
                self.offset_left = parse_offset(val);
            }
            "box-sizing" => {
                self.box_sizing = match val {
                    "border-box" => BoxSizing::BorderBox,
                    _ => BoxSizing::ContentBox,
                };
            }
            "min-width" => {
                self.min_width = parse_length(val);
            }
            "max-width" => {
                self.max_width = parse_length(val);
            }
            "grid-template-columns" => {
                self.grid_template_columns = parse_grid_track_list(val);
            }
            "grid-template-rows" => {
                self.grid_template_rows = parse_grid_track_list(val);
            }
            "grid-column-gap" => {
                self.grid_column_gap = parse_length(val).unwrap_or(0.0);
            }
            "grid-row-gap" => {
                self.grid_row_gap = parse_length(val).unwrap_or(0.0);
            }
            "order" => {
                self.order = val.parse().unwrap_or(0);
            }
            "align-self" => {
                self.align_self = parse_align_self(val);
            }
            "border" => {
                let (w, c) = parse_border_shorthand(val);
                if let Some(width) = w {
                    self.border_width = [width; 4];
                }
                if let Some(color) = c {
                    self.border_color = Some(color);
                }
            }
            "border-top" => {
                let (w, c) = parse_border_shorthand(val);
                if let Some(width) = w {
                    self.border_width[0] = width;
                }
                if let Some(color) = c {
                    self.border_color = Some(color);
                }
            }
            "border-right" => {
                let (w, c) = parse_border_shorthand(val);
                if let Some(width) = w {
                    self.border_width[1] = width;
                }
                if let Some(color) = c {
                    self.border_color = Some(color);
                }
            }
            "border-bottom" => {
                let (w, c) = parse_border_shorthand(val);
                if let Some(width) = w {
                    self.border_width[2] = width;
                }
                if let Some(color) = c {
                    self.border_color = Some(color);
                }
            }
            "border-left" => {
                let (w, c) = parse_border_shorthand(val);
                if let Some(width) = w {
                    self.border_width[3] = width;
                }
                if let Some(color) = c {
                    self.border_color = Some(color);
                }
            }
            "border-width" => {
                self.border_width = parse_box_four(val, self.border_width);
            }
            "border-color" => {
                if let Some(c) = parse_color_value(val) {
                    let (r, g, b, a) = c.to_rgba();
                    self.border_color = Some([r, g, b, a]);
                }
            }
            "border-radius" => {
                if let Some(r) = parse_length(val) {
                    self.border_radius = r;
                }
            }
            "font-weight" => {
                self.text_style.bold = parse_is_bold(val);
            }
            "font-style" => {
                self.text_style.italic =
                    matches!(val.to_ascii_lowercase().as_str(), "italic" | "oblique");
            }
            "text-decoration" | "text-decoration-line" => {
                // The shorthand also carries color/style/thickness, which we
                // ignore; only the line keywords affect what we draw. `none`
                // clears any decoration inherited from an earlier declaration.
                let lower = val.to_ascii_lowercase();
                self.text_style.underline = lower.contains("underline");
                self.text_style.line_through = lower.contains("line-through");
                self.text_style.overline = lower.contains("overline");
            }
            "visibility" => {
                self.visibility = match val.to_ascii_lowercase().as_str() {
                    "hidden" => Visibility::Hidden,
                    "collapse" => Visibility::Collapse,
                    _ => Visibility::Visible,
                };
            }
            "letter-spacing" => {
                // `normal` is the initial value and means no extra tracking.
                self.text_style.letter_spacing = if val.eq_ignore_ascii_case("normal") {
                    0.0
                } else {
                    parse_length(val).unwrap_or(0.0)
                };
            }
            "word-spacing" => {
                self.text_style.word_spacing = if val.eq_ignore_ascii_case("normal") {
                    0.0
                } else {
                    parse_length(val).unwrap_or(0.0)
                };
            }
            "text-transform" => {
                self.text_style.transform = match val.to_ascii_lowercase().as_str() {
                    "uppercase" => TextTransform::Uppercase,
                    "lowercase" => TextTransform::Lowercase,
                    "capitalize" => TextTransform::Capitalize,
                    _ => TextTransform::None,
                };
            }
            "text-align" => {
                self.text_align = match val {
                    "center" => TextAlign::Center,
                    "right" => TextAlign::Right,
                    "justify" => TextAlign::Justify,
                    _ => TextAlign::Left,
                };
            }
            _ => {}
        }

        self
    }
}

/// Helper to parse CSS border shorthand like "1px solid #ccc"
fn parse_border_shorthand(val: &str) -> (Option<f32>, Option<[u8; 4]>) {
    let mut width = None;
    let mut color = None;
    for part in val.split_whitespace() {
        if width.is_none() {
            if let Some(w) = parse_length(part) {
                width = Some(w);
                continue;
            }
        }
        if color.is_none() {
            if let Some(c) = parse_color_value(part) {
                let (r, g, b, a) = c.to_rgba();
                color = Some([r, g, b, a]);
            }
        }
    }
    (width, color)
}

/// Parse a CSS offset value (top/right/bottom/left) allowing negative pixels.
/// Returns `None` for `"auto"`, `"inherit"`, or unparseable values.
fn parse_offset(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto")
        || s.eq_ignore_ascii_case("inherit")
        || s.eq_ignore_ascii_case("static")
    {
        return None;
    }
    // Strip common unit suffixes, keeping the sign
    let mut chars = s.chars().peekable();
    // Collect up to the first alphabetic char (unit suffix)
    let mut num_str = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphabetic() {
            break;
        }
        num_str.push(c);
        chars.next();
    }
    // Trim whitespace from the collected number string
    let num_str = num_str.trim();
    if num_str.is_empty() {
        return None;
    }
    num_str.parse::<f32>().ok()
}

/// The context needed to resolve relative CSS length units into pixels.
///
/// `em` resolves against the *current* element's font size, `rem` against the
/// root (`<html>`) font size, and the viewport units against the viewport box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LengthContext {
    /// Font size of the element the declaration applies to (for `em`, `ex`, `ch`).
    pub font_size: f32,
    /// Font size of the root `<html>` element (for `rem`).
    pub root_font_size: f32,
    /// Viewport width in CSS pixels (for `vw`, `vmin`, `vmax`).
    pub viewport_width: f32,
    /// Viewport height in CSS pixels (for `vh`, `vmin`, `vmax`).
    pub viewport_height: f32,
}

/// The CSS initial font size, used whenever no explicit context is available.
pub const DEFAULT_FONT_SIZE: f32 = 16.0;

impl Default for LengthContext {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            root_font_size: DEFAULT_FONT_SIZE,
            viewport_width: 800.0,
            viewport_height: 600.0,
        }
    }
}

/// Parse a CSS length value into pixels, resolving relative units via `ctx`.
///
/// Supports absolute units (`px`, `pt`, `pc`, `in`, `cm`, `mm`, `q`), font-relative
/// units (`em`, `rem`, `ex`, `ch`), and viewport units (`vw`, `vh`, `vmin`, `vmax`).
/// A bare number is treated as pixels (quirks-friendly, and correct for `0`).
/// Returns `None` for `"auto"`, `"inherit"`, percentages, or unparseable values.
fn parse_length_ctx(s: &str, ctx: LengthContext) -> Option<f32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("inherit") {
        return None;
    }
    // Percentages are resolved by the layout engine against a containing block,
    // so they are not a plain length here.
    if s.ends_with('%') {
        return None;
    }

    // Split the numeric part from the trailing unit.
    let unit_start = s
        .rfind(|c: char| c.is_ascii_digit() || c == '.')
        .map(|i| i + 1)
        .unwrap_or(0);
    let (num_str, unit) = s.split_at(unit_start);
    let num: f32 = num_str.trim().parse().ok()?;
    if !num.is_finite() {
        return None;
    }

    let unit = unit.trim().to_ascii_lowercase();
    let px = match unit.as_str() {
        "" | "px" => num,
        "em" => num * ctx.font_size,
        "rem" => num * ctx.root_font_size,
        // Approximations used by every engine when real font metrics are unavailable.
        "ex" => num * ctx.font_size * 0.5,
        "ch" => num * ctx.font_size * 0.5,
        "vw" => num * ctx.viewport_width / 100.0,
        "vh" => num * ctx.viewport_height / 100.0,
        "vmin" => num * ctx.viewport_width.min(ctx.viewport_height) / 100.0,
        "vmax" => num * ctx.viewport_width.max(ctx.viewport_height) / 100.0,
        // Absolute units, defined by CSS against 96dpi.
        "pt" => num * 96.0 / 72.0,
        "pc" => num * 16.0,
        "in" => num * 96.0,
        "cm" => num * 96.0 / 2.54,
        "mm" => num * 96.0 / 25.4,
        "q" => num * 96.0 / 101.6,
        _ => return None,
    };
    Some(px)
}

/// Parse a CSS length using the default context (16px font, 800x600 viewport).
fn parse_length(s: &str) -> Option<f32> {
    parse_length_ctx(s, LengthContext::default())
}

/// Parse margin shorthand supporting `auto` values.
fn parse_margin_box_four(
    s: &str,
    fallback_margin: [f32; 4],
    fallback_auto: [bool; 4],
    ctx: LengthContext,
) -> ([f32; 4], [bool; 4]) {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut parsed = Vec::new();
    for t in tokens {
        if t.eq_ignore_ascii_case("auto") {
            parsed.push((0.0, true));
        } else if let Some(v) = parse_length_ctx(t, ctx) {
            parsed.push((v, false));
        }
    }

    match parsed.len() {
        1 => ([parsed[0].0; 4], [parsed[0].1; 4]),
        2 => (
            [parsed[0].0, parsed[1].0, parsed[0].0, parsed[1].0],
            [parsed[0].1, parsed[1].1, parsed[0].1, parsed[1].1],
        ),
        3 => (
            [parsed[0].0, parsed[1].0, parsed[2].0, parsed[1].0],
            [parsed[0].1, parsed[1].1, parsed[2].1, parsed[1].1],
        ),
        4 => (
            [parsed[0].0, parsed[1].0, parsed[2].0, parsed[3].0],
            [parsed[0].1, parsed[1].1, parsed[2].1, parsed[3].1],
        ),
        _ => (fallback_margin, fallback_auto),
    }
}

/// Parse a box model shorthand (margin/padding) into four values.
///
/// Supports 1, 2, 3, or 4 space-separated values:
/// - 1 value: all sides
/// - 2 values: vertical, horizontal
/// - 3 values: top, horizontal, bottom
/// - 4 values: top, right, bottom, left
fn parse_box_four(s: &str, fallback: [f32; 4], ctx: LengthContext) -> [f32; 4] {
    let parts: Vec<f32> = s
        .split_whitespace()
        .filter_map(|p| parse_length_ctx(p, ctx))
        .collect();

    match parts.len() {
        1 => [parts[0]; 4],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        4 => [parts[0], parts[1], parts[2], parts[3]],
        _ => fallback,
    }
}

/// Parse flex-basis value: "auto" -> FlexBasis::Auto, numeric length -> FlexBasis::Pixels(px),
/// percentage value (e.g. "50%") -> FlexBasis::Percentage(frac).
fn parse_flex_basis(s: &str, ctx: LengthContext) -> FlexBasis {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("content") {
        return FlexBasis::Auto;
    }
    if let Some(pct_val) = s.strip_suffix('%') {
        if let Ok(n) = pct_val.parse::<f32>() {
            return FlexBasis::Percentage(n / 100.0);
        }
    }
    if let Some(px) = parse_length_ctx(s, ctx) {
        return FlexBasis::Pixels(px);
    }
    FlexBasis::Auto
}

/// Parse the flex shorthand property: "flex: <grow> <shrink>? <basis>?"
fn parse_flex_shorthand(self_vals: &mut ComputedValues, val: &str, ctx: LengthContext) {
    let parts: Vec<&str> = val.trim().split_whitespace().collect();

    if parts.is_empty() {
        return;
    }

    // Special case: "flex: none" -> grow=0, shrink=0, basis=0px
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
        self_vals.flex_grow = 0.0;
        self_vals.flex_shrink = 0.0;
        self_vals.flex_basis = FlexBasis::Pixels(0.0);
        return;
    }

    // "flex: auto" -> grow=1, shrink=1, basis=auto
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("auto") {
        self_vals.flex_grow = 1.0;
        self_vals.flex_shrink = 1.0;
        self_vals.flex_basis = FlexBasis::Auto;
        return;
    }

    // Single number: only grow is set (shrink stays default 1, basis stays auto)
    if parts.len() == 1 {
        if let Ok(v) = parts[0].parse::<f32>() {
            self_vals.flex_grow = v;
        }
        return;
    }

    // Two numbers: grow shrink (or grow basis if second is a length)
    if parts.len() == 2 {
        // First part is always flex-grow
        if let Ok(grow) = parts[0].parse::<f32>() {
            self_vals.flex_grow = grow;
        }
        // Second part: try as number (shrink), else try as length/percentage (basis)
        if let Ok(shrink) = parts[1].parse::<f32>() {
            self_vals.flex_shrink = shrink;
        } else {
            self_vals.flex_basis = parse_flex_basis(parts[1], ctx);
        }
        return;
    }

    // Three parts: grow shrink basis
    if parts.len() == 3 {
        if let Ok(grow) = parts[0].parse::<f32>() {
            self_vals.flex_grow = grow;
        }
        if let Ok(shrink) = parts[1].parse::<f32>() {
            self_vals.flex_shrink = shrink;
        }
        self_vals.flex_basis = parse_flex_basis(parts[2], ctx);
    }
}

/// Parse align-content value.
fn parse_align_content(s: &str) -> AlignContent {
    match s.trim() {
        "normal" => AlignContent::Normal,
        "flex-start" => AlignContent::FlexStart,
        "flex-end" => AlignContent::FlexEnd,
        "center" => AlignContent::Center,
        "space-between" => AlignContent::SpaceBetween,
        "space-around" => AlignContent::SpaceAround,
        "stretch" => AlignContent::Stretch,
        _ => AlignContent::Normal,
    }
}

/// Parse align-self value.
fn parse_align_self(s: &str) -> AlignSelf {
    match s.trim() {
        "auto" => AlignSelf::Auto,
        "flex-start" | "start" => AlignSelf::FlexStart,
        "flex-end" | "end" => AlignSelf::FlexEnd,
        "center" => AlignSelf::Center,
        "baseline" => AlignSelf::Baseline,
        "stretch" => AlignSelf::Stretch,
        _ => AlignSelf::Auto,
    }
}

/// Parse a single track token string (e.g. "1fr", "200px", "min-content", "max-content", "auto", "fit-content(100px)")
fn parse_single_grid_track(token: &str, ctx: LengthContext) -> Option<GridTrack> {
    let token = token.trim();
    if token.eq_ignore_ascii_case("auto") {
        return Some(GridTrack::Auto);
    }
    if token.eq_ignore_ascii_case("min-content") {
        return Some(GridTrack::MinContent);
    }
    if token.eq_ignore_ascii_case("max-content") {
        return Some(GridTrack::MaxContent);
    }
    if let Some(fit) = token
        .strip_prefix("fit-content(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let fit_px = parse_length_ctx(fit.trim(), ctx).unwrap_or(0.0);
        return Some(GridTrack::FitContent(fit_px));
    }
    if let Some(fr_val) = token.strip_suffix("fr") {
        if let Ok(n) = fr_val.parse::<f32>() {
            return Some(GridTrack::Fr(n));
        }
    }
    if let Some(px) = parse_length_ctx(token, ctx) {
        return Some(GridTrack::Fixed(px));
    }
    None
}

/// Parse a grid track list like "1fr 1fr 200px auto min-content repeat(3, 1fr)" into GridTrack enums.
fn parse_grid_track_list(value: &str, ctx: LengthContext) -> Vec<GridTrack> {
    let mut tracks = Vec::new();
    let val_trimmed = value.trim();
    if val_trimmed.is_empty() {
        return tracks;
    }

    // Handle repeat(...) syntax: e.g. repeat(3, 1fr) or repeat(2, min-content 1fr)
    let mut chars = val_trimmed.chars().peekable();
    let mut token = String::new();
    let mut paren_depth = 0usize;

    while let Some(ch) = chars.next() {
        if ch == '(' {
            paren_depth += 1;
            token.push(ch);
        } else if ch == ')' {
            if paren_depth > 0 {
                paren_depth -= 1;
            }
            token.push(ch);
            if paren_depth == 0 {
                let trimmed_tok = token.trim();
                if let Some(repeat_content) = trimmed_tok
                    .strip_prefix("repeat(")
                    .and_then(|s| s.strip_suffix(')'))
                {
                    if let Some((count_str, track_str)) = repeat_content.split_once(',') {
                        if let Ok(count) = count_str.trim().parse::<usize>() {
                            let sub_tracks = parse_grid_track_list(track_str, ctx);
                            for _ in 0..count.min(100) {
                                tracks.extend(sub_tracks.iter().cloned());
                            }
                        }
                    }
                } else if let Some(tr) = parse_single_grid_track(trimmed_tok, ctx) {
                    tracks.push(tr);
                }
                token.clear();
            }
        } else if ch.is_whitespace() && paren_depth == 0 {
            if !token.is_empty() {
                if let Some(tr) = parse_single_grid_track(&token, ctx) {
                    tracks.push(tr);
                }
                token.clear();
            }
        } else {
            token.push(ch);
        }
    }

    if !token.is_empty() {
        let trimmed_tok = token.trim();
        if let Some(repeat_content) = trimmed_tok
            .strip_prefix("repeat(")
            .and_then(|s| s.strip_suffix(')'))
        {
            if let Some((count_str, track_str)) = repeat_content.split_once(',') {
                if let Ok(count) = count_str.trim().parse::<usize>() {
                    let sub_tracks = parse_grid_track_list(track_str, ctx);
                    for _ in 0..count.min(100) {
                        tracks.extend(sub_tracks.iter().cloned());
                    }
                }
            }
        } else if let Some(tr) = parse_single_grid_track(trimmed_tok, ctx) {
            tracks.push(tr);
        }
    }

    tracks
}

// ------ User-Agent Stylesheet ------

/// Returns a default user-agent stylesheet with standard UA styles for common HTML elements.
///
/// These rules have the lowest priority in the CSS cascade — author styles always override them.
pub fn user_agent_stylesheet() -> parser::Stylesheet {
    // Each tuple: (selector, declarations_string)
    let ua_rules = vec![
        ("html", "display: block"),
        ("body", "display: block; margin: 8px"),
        (
            "h1",
            "display: block; font-size: 2em; margin: 0.67em 0; font-weight: bold",
        ),
        (
            "h2",
            "display: block; font-size: 1.5em; margin: 0.83em 0; font-weight: bold",
        ),
        (
            "h3",
            "display: block; font-size: 1.17em; margin: 1em 0; font-weight: bold",
        ),
        ("h4", "display: block; margin: 1em 0; font-weight: bold"),
        ("h5", "display: block; margin: 1em 0; font-weight: bold"),
        ("h6", "display: block; margin: 1em 0; font-weight: bold"),
        ("p", "display: block; margin: 1em 0"),
        ("ul", "display: block; padding-left: 40px; margin: 1em 0"),
        ("ol", "display: block; padding-left: 40px; margin: 1em 0"),
        ("li", "display: block"),
        (
            "table",
            "display: table; border-collapse: collapse; margin: 0.5em 0",
        ),
        ("thead", "display: table-header-group"),
        ("tbody", "display: table-row-group"),
        ("tfoot", "display: table-footer-group"),
        ("tr", "display: table-row"),
        (
            "td",
            "display: table-cell; padding: 4px 8px; border: 1px solid #a2a9b1",
        ),
        (
            "th",
            "display: table-cell; font-weight: bold; padding: 4px 8px; border: 1px solid #c8ccd1; background-color: #eaecf0; text-align: center",
        ),
        ("caption", "display: table-caption"),
        ("colgroup", "display: table-column-group"),
        ("col", "display: table-column"),
        ("img", "display: inline"),
        (
            "a",
            "display: inline; color: #0645ad; text-decoration: underline",
        ),
        ("div", "display: block"),
        ("center", "display: block; text-align: center"),
        ("main", "display: block"),
        ("header", "display: block"),
        ("footer", "display: block"),
        ("nav", "display: block"),
        ("section", "display: block"),
        ("article", "display: block"),
        ("aside", "display: block"),
        ("figure", "display: block; margin: 1em 40px"),
        ("figcaption", "display: block"),
        ("span", "display: inline"),
        ("br", "display: inline"),
        // Inline text semantics. Without these the cascade never sets the
        // bold/italic/decoration flags, so real markup renders flat.
        ("b", "display: inline; font-weight: bold"),
        ("strong", "display: inline; font-weight: bold"),
        ("i", "display: inline; font-style: italic"),
        ("em", "display: inline; font-style: italic"),
        ("cite", "display: inline; font-style: italic"),
        ("var", "display: inline; font-style: italic"),
        ("dfn", "display: inline; font-style: italic"),
        ("address", "display: block; font-style: italic"),
        ("u", "display: inline; text-decoration: underline"),
        ("ins", "display: inline; text-decoration: underline"),
        ("s", "display: inline; text-decoration: line-through"),
        ("strike", "display: inline; text-decoration: line-through"),
        ("del", "display: inline; text-decoration: line-through"),
        (
            "a",
            "display: inline; text-decoration: underline; color: #0000ee",
        ),
        ("small", "display: inline; font-size: 0.8em"),
        ("big", "display: inline; font-size: 1.2em"),
        ("code", "display: inline; font-family: monospace"),
        ("kbd", "display: inline; font-family: monospace"),
        ("samp", "display: inline; font-family: monospace"),
        (
            "pre",
            "display: block; font-family: monospace; margin: 1em 0",
        ),
        ("mark", "display: inline; background-color: #ffff00"),
        ("form", "display: block"),
        (
            "input",
            "display: inline-block; padding: 4px 8px; border: 1px solid #767676; border-radius: 2px; background-color: #ffffff; color: #000000",
        ),
        (
            "button",
            "display: inline-block; padding: 4px 12px; border: 1px solid #767676; border-radius: 2px; background-color: #f0f0f0; color: #000000; font-weight: bold",
        ),
        (
            "textarea",
            "display: inline-block; padding: 4px 8px; border: 1px solid #767676; border-radius: 2px; background-color: #ffffff",
        ),
        ("head", "display: none"),
        ("script", "display: none"),
        ("style", "display: none"),
        ("meta", "display: none"),
        ("title", "display: none"),
        ("link", "display: none"),
        ("noscript", "display: none"),
    ];

    let mut rules = Vec::new();

    for (selector_str, decls_str) in ua_rules {
        let selectors = parser::parse_selector_str(selector_str);
        let declarations = parse_declarations(decls_str);
        rules.push(parser::CSSRule {
            selectors,
            declarations,
        });
    }

    parser::Stylesheet {
        rules,
        imports: Vec::new(),
        media_rules: Vec::new(),
    }
}

/// Merges UA stylesheet rules with author stylesheet rules.
///
/// In CSS cascade, earlier source order loses to later source order at equal specificity,
/// so UA rules are prepended and author rules are appended — author styles win.
pub fn merge_stylesheets_with_author(
    ua: &parser::Stylesheet,
    author: &parser::Stylesheet,
) -> parser::Stylesheet {
    let mut rules = Vec::new();
    rules.extend_from_slice(&ua.rules);
    rules.extend_from_slice(&author.rules);
    // UA stylesheet has no imports; pass through author imports (already resolved at fetch time)
    let imports = author.imports.clone();
    parser::Stylesheet {
        rules,
        imports,
        media_rules: author.media_rules.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_declarations_single() {
        let decls = parse_declarations("color: red");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn parse_declarations_multiple() {
        let decls = parse_declarations("color: red; margin: 10px");
        assert_eq!(decls.len(), 2);
    }

    #[test]
    fn parse_empty_declarations() {
        let decls = parse_declarations("");
        assert!(decls.is_empty());
    }

    #[test]
    fn parse_declarations_important() {
        let decls = parse_declarations("color: red !important");
        assert_eq!(decls.len(), 1);
        assert!(decls[0].important);
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn parse_color_red() {
        assert_eq!(
            parse_color_value("red"),
            Some(CSSColor::Named("red".to_string()))
        );
    }

    #[test]
    fn parse_color_hex() {
        assert_eq!(
            parse_color_value("#ff0000"),
            Some(CSSColor::Hex { r: 255, g: 0, b: 0 })
        );
    }

    #[test]
    fn parse_color_hex_short() {
        assert_eq!(
            parse_color_value("#f00"),
            Some(CSSColor::Hex { r: 255, g: 0, b: 0 })
        );
    }

    #[test]
    fn parse_color_invalid() {
        assert!(parse_color_value("invalid").is_none());
    }

    // ------ rgb()/rgba() Parsing Tests ------

    #[test]
    fn parse_color_rgb_basic() {
        let result = parse_color_value("rgb(255, 0, 0)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgb_white() {
        let result = parse_color_value("rgb(255, 255, 255)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgb_black() {
        let result = parse_color_value("rgb(0, 0, 0)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgba_full_alpha() {
        let result = parse_color_value("rgba(0, 0, 255, 1.0)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 0,
                g: 0,
                b: 255,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgba_half_alpha() {
        let result = parse_color_value("rgba(0, 0, 255, 0.5)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 0,
                g: 0,
                b: 255,
                a: 0.5
            })
        );
    }

    #[test]
    fn parse_color_rgba_zero_alpha() {
        let result = parse_color_value("rgba(255, 128, 0, 0.0)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 255,
                g: 128,
                b: 0,
                a: 0.0
            })
        );
    }

    #[test]
    fn parse_color_rgb_with_spaces() {
        // Extra whitespace inside parentheses
        let result = parse_color_value("rgb( 100 , 200 , 50 )");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 100,
                g: 200,
                b: 50,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgb_clamp_over() {
        // Values > 255 should clamp to 255
        let result = parse_color_value("rgb(300, -10, 256)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 255,
                g: 0,
                b: 255,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgba_clamp_alpha() {
        // Alpha > 1.0 or < 0.0 should be rejected
        assert!(parse_color_value("rgba(0, 0, 0, 1.5)").is_none());
        assert!(parse_color_value("rgba(0, 0, 0, -0.1)").is_none());
    }

    #[test]
    fn parse_color_rgb_percentage() {
        // rgb(100%, 0%, 0%) should be red
        let result = parse_color_value("rgb(100%, 0%, 0%)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgb_percentage_50() {
        // rgb(50%, 50%, 50%) should be gray (~128 each)
        let result = parse_color_value("rgb(50%, 50%, 50%)");
        assert_eq!(
            result,
            Some(CSSColor::Rgba {
                r: 127,
                g: 127,
                b: 127,
                a: 1.0
            })
        );
    }

    #[test]
    fn parse_color_rgb_to_rgba_conversion() {
        // rgb(100, 200, 50).to_rgba() should have alpha=255
        let result = parse_color_value("rgb(100, 200, 50)");
        assert_eq!(result.unwrap().to_rgba(), (100, 200, 50, 255));
    }

    #[test]
    fn parse_color_rgba_to_rgba_conversion() {
        // rgba(100, 200, 50, 0.5).to_rgba() should have alpha=127
        let result = parse_color_value("rgba(100, 200, 50, 0.5)");
        assert_eq!(result.unwrap().to_rgba(), (100, 200, 50, 127));
    }

    #[test]
    fn parse_color_rgb_invalid_too_few_args() {
        assert!(parse_color_value("rgb(255, 0)").is_none());
    }

    #[test]
    fn parse_color_rgb_invalid_non_numeric() {
        assert!(parse_color_value("rgb(foo, bar, baz)").is_none());
    }

    #[test]
    fn parse_color_rgba_case_insensitive() {
        // RGB(), RGBA(), Rgb() should all work (case-insensitive)
        assert!(parse_color_value("RGB(10, 20, 30)").is_some());
        assert!(parse_color_value("RGBA(10, 20, 30, 0.5)").is_some());
        assert!(parse_color_value("Rgb(40, 50, 60)").is_some());
    }

    // ------ Cascade & Inheritance Tests ------

    #[test]
    fn specificity_type_selector() {
        use crate::css::parser::{Selector, SimpleSelector};
        let sel = Selector::simple(SimpleSelector::Type("div".to_string()));
        assert_eq!(sel.specificity(), (0, 0, 1));
    }

    #[test]
    fn specificity_class_selector() {
        use crate::css::parser::{Selector, SimpleSelector};
        let sel = Selector::simple(SimpleSelector::Class("btn".to_string()));
        assert_eq!(sel.specificity(), (0, 1, 0));
    }

    #[test]
    fn specificity_id_selector() {
        use crate::css::parser::{Selector, SimpleSelector};
        let sel = Selector::simple(SimpleSelector::Id("main".to_string()));
        assert_eq!(sel.specificity(), (1, 0, 0));
    }

    #[test]
    fn specificity_universal() {
        use crate::css::parser::{Selector, SimpleSelector};
        let sel = Selector::simple(SimpleSelector::Universal);
        assert_eq!(sel.specificity(), (0, 0, 0));
    }

    #[test]
    fn specificity_complex_selector() {
        use crate::css::parser::{Combinator, Selector, SimpleSelector};
        let mut sel = Selector::simple(SimpleSelector::Type("div".to_string()));
        sel.push(
            Combinator::Descendant,
            SimpleSelector::Class("highlight".to_string()),
        );
        sel.push(Combinator::Child, SimpleSelector::Id("content".to_string()));
        // 1 ID + 1 class + 1 type
        assert_eq!(sel.specificity(), (1, 1, 1));
    }

    #[test]
    fn cascade_basic_match() {
        let arena = crate::html::parse_html("<div>Hello</div>");
        let stylesheet = parser::parse_stylesheet("div { color: red; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        // The <div> should have a computed style entry
        assert!(!styles.is_empty());

        // Find the <div> (it's the body's child at index 0, but we check by ID)
        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        assert!(div_id.is_some(), "Expected to find a <div> node");

        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // red as RGBA
    }

    #[test]
    fn cascade_specificity_wins() {
        // ID selector (#mydiv) has higher specificity than type selector (div)
        let arena = crate::html::parse_html(r#"<div id="mydiv">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { color: blue; } #mydiv { color: red; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        // ID selector (#mydiv) wins → red
        assert_eq!(div_styles.color, Some([255, 0, 0, 255]));
    }

    #[test]
    fn cascade_important_wins() {
        // !important declaration overrides higher specificity
        let arena = crate::html::parse_html(r#"<div id="x">Test</div>"#);
        let stylesheet =
            parser::parse_stylesheet("div { color: blue !important; } #x { color: red; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        // !important wins regardless of specificity → blue
        assert_eq!(div_styles.color, Some([0, 0, 255, 255]));
    }

    #[test]
    fn cascade_source_order() {
        // Same specificity: later rule wins
        let arena = crate::html::parse_html(r#"<div class="a">Test</div>"#);
        let stylesheet = parser::parse_stylesheet(".a { color: red; } .a { color: blue; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        // Later rule wins → blue
        assert_eq!(div_styles.color, Some([0, 0, 255, 255]));
    }

    #[test]
    fn inheritance_color_propagates() {
        // color property should inherit from parent to child
        let arena = crate::html::parse_html("<div style='color:red'><span>child</span></div>");
        // Style only on <div>; <span> should inherit
        let stylesheet = parser::parse_stylesheet("div { color: red; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let span_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "span")
                    .unwrap_or(false)
        });
        if let Some(sid) = span_id {
            let span_styles = styles.get(&(sid as u32)).expect("span has styles");
            assert_eq!(
                span_styles.color,
                Some([255, 0, 0, 255]),
                "color should inherit"
            );
        } else {
            // html5ever may or may not create a <span> node depending on parsing
            assert!(false, "Expected to find a <span> node");
        }
    }

    #[test]
    fn inheritance_font_size_propagates() {
        let arena = crate::html::parse_html("<div><p>text</p></div>");
        let stylesheet = parser::parse_stylesheet("div { font-size: 24px; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let p_id = nodes.iter().position(|n| {
            n.is_element() && n.tag_name().map(|t| t.to_string() == "p").unwrap_or(false)
        });
        if let Some(pid) = p_id {
            let p_styles = styles.get(&(pid as u32)).expect("p has styles");
            assert_eq!(p_styles.font_size, 24.0, "font-size should inherit");
        } else {
            assert!(false, "Expected to find a <p> node");
        }
    }

    #[test]
    fn defaults_applied() {
        let arena = crate::html::parse_html("<div></div>");
        let stylesheet = parser::parse_stylesheet(""); // empty stylesheet
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        assert!(!styles.is_empty());
        // All element nodes should have default ComputedValues
        for (_id, values) in &styles {
            assert_eq!(values.font_size, 16.0, "Default font-size is 16px");
            assert_eq!(values.margin, [0.0; 4], "Default margin is 0");
        }
    }

    // ------ Flexbox Property Parsing Tests ------

    #[test]
    fn test_parse_display_flex() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "display".to_string(),
            value: "flex".to_string(),
            important: false,
        });
        assert_eq!(computed.display, DisplayType::Flex);
    }

    #[test]
    fn test_parse_display_inline_flex() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "display".to_string(),
            value: "inline-flex".to_string(),
            important: false,
        });
        assert_eq!(computed.display, DisplayType::InlineFlex);
    }

    #[test]
    fn test_parse_flex_direction_column() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex-direction".to_string(),
            value: "column".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn test_parse_justify_content_space_between() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "justify-content".to_string(),
            value: "space-between".to_string(),
            important: false,
        });
        assert_eq!(computed.justify_content, JustifyContent::SpaceBetween);
    }

    #[test]
    fn test_parse_align_items_center() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "align-items".to_string(),
            value: "center".to_string(),
            important: false,
        });
        assert_eq!(computed.align_items, AlignItems::Center);
    }

    #[test]
    fn test_parse_flex_shorthand_full() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex".to_string(),
            value: "2 1 100px".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_grow, 2.0);
        assert_eq!(computed.flex_shrink, 1.0);
        assert_eq!(computed.flex_basis, FlexBasis::Pixels(100.0));
    }

    #[test]
    fn test_parse_flex_shorthand_none() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex".to_string(),
            value: "none".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_grow, 0.0);
        assert_eq!(computed.flex_shrink, 0.0);
        assert_eq!(computed.flex_basis, FlexBasis::Pixels(0.0));
    }

    #[test]
    fn test_parse_flex_shorthand_auto() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex".to_string(),
            value: "auto".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_grow, 1.0);
        assert_eq!(computed.flex_shrink, 1.0);
        assert_eq!(computed.flex_basis, FlexBasis::Auto);
    }

    #[test]
    fn test_parse_flex_shorthand_grow_only() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex".to_string(),
            value: "3".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_grow, 3.0);
        assert_eq!(computed.flex_shrink, 1.0); // default
        assert_eq!(computed.flex_basis, FlexBasis::Auto); // default
    }

    #[test]
    fn test_flex_defaults() {
        let computed = ComputedValues::default();
        assert_eq!(computed.flex_direction, FlexDirection::Row);
        assert_eq!(computed.flex_wrap, FlexWrap::NoWrap);
        assert_eq!(computed.justify_content, JustifyContent::FlexStart);
        assert_eq!(computed.align_items, AlignItems::Stretch);
        assert_eq!(computed.align_content, AlignContent::Normal);
        assert_eq!(computed.row_gap, 0.0);
        assert_eq!(computed.column_gap, 0.0);
        assert_eq!(computed.flex_grow, 0.0);
        assert_eq!(computed.flex_shrink, 1.0);
        assert_eq!(computed.flex_basis, FlexBasis::Auto);
    }

    #[test]
    fn test_parse_align_content_center() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "align-content".to_string(),
            value: "center".to_string(),
            important: false,
        });
        assert_eq!(computed.align_content, AlignContent::Center);
    }

    #[test]
    fn test_parse_align_content_space_between() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "align-content".to_string(),
            value: "space-between".to_string(),
            important: false,
        });
        assert_eq!(computed.align_content, AlignContent::SpaceBetween);
    }

    #[test]
    fn test_parse_gap_single_value() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "gap".to_string(),
            value: "16px".to_string(),
            important: false,
        });
        assert_eq!(computed.row_gap, 16.0);
        assert_eq!(computed.column_gap, 16.0);
    }

    #[test]
    fn test_parse_gap_two_values() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "gap".to_string(),
            value: "10px 20px".to_string(),
            important: false,
        });
        assert_eq!(computed.row_gap, 10.0);
        assert_eq!(computed.column_gap, 20.0);
    }

    #[test]
    fn test_parse_row_gap() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "row-gap".to_string(),
            value: "8px".to_string(),
            important: false,
        });
        assert_eq!(computed.row_gap, 8.0);
        assert_eq!(computed.column_gap, 0.0); // column_gap stays default
    }

    #[test]
    fn test_parse_column_gap() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "column-gap".to_string(),
            value: "12px".to_string(),
            important: false,
        });
        assert_eq!(computed.row_gap, 0.0); // row_gap stays default
        assert_eq!(computed.column_gap, 12.0);
    }

    #[test]
    fn test_parse_flex_wrap_wrap() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex-wrap".to_string(),
            value: "wrap".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_wrap, FlexWrap::Wrap);
    }

    #[test]
    fn test_parse_flex_wrap_reverse() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex-wrap".to_string(),
            value: "wrap-reverse".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_wrap, FlexWrap::WrapReverse);
    }

    // ------ Overflow & Positioning Tests ------

    #[test]
    fn test_overflow_enum_visible() {
        // Overflow enum round-trip through parsing
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "overflow".to_string(),
            value: "hidden".to_string(),
            important: false,
        });
        assert_eq!(computed.overflow_x, Overflow::Hidden);
        assert_eq!(computed.overflow_y, Overflow::Hidden);

        // Test "visible" via overflow-x only
        let computed2 = ComputedValues::default().from_declaration(&Declaration {
            property: "overflow-x".to_string(),
            value: "scroll".to_string(),
            important: false,
        });
        assert_eq!(computed2.overflow_x, Overflow::Scroll);
        assert_eq!(computed2.overflow_y, Overflow::Visible); // unchanged default
    }

    #[test]
    fn test_position_type_relative() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "position".to_string(),
            value: "relative".to_string(),
            important: false,
        });
        assert_eq!(computed.position, PositionType::Relative);

        let computed2 = ComputedValues::default().from_declaration(&Declaration {
            property: "position".to_string(),
            value: "absolute".to_string(),
            important: false,
        });
        assert_eq!(computed2.position, PositionType::Absolute);

        // Default is static
        let computed3 = ComputedValues::default();
        assert_eq!(computed3.position, PositionType::Static);
    }

    #[test]
    fn test_offset_negative_left() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "left".to_string(),
            value: "-10px".to_string(),
            important: false,
        });
        assert_eq!(computed.offset_left, Some(-10.0));

        // Test top with positive value
        let computed2 = ComputedValues::default().from_declaration(&Declaration {
            property: "top".to_string(),
            value: "20px".to_string(),
            important: false,
        });
        assert_eq!(computed2.offset_top, Some(20.0));

        // Test bottom
        let computed3 = ComputedValues::default().from_declaration(&Declaration {
            property: "bottom".to_string(),
            value: "-5px".to_string(),
            important: false,
        });
        assert_eq!(computed3.offset_bottom, Some(-5.0));

        // Test right
        let computed4 = ComputedValues::default().from_declaration(&Declaration {
            property: "right".to_string(),
            value: "auto".to_string(),
            important: false,
        });
        assert_eq!(computed4.offset_right, None); // "auto" maps to None
    }

    #[test]
    fn test_overflow_hidden_not_inherited() {
        // Overflow should NOT inherit from parent to child per CSS spec.
        // Parent has overflow:hidden; child should have default visible.
        let arena = crate::html::parse_html("<div><p>child</p></div>");
        let stylesheet = parser::parse_stylesheet("div { overflow: hidden; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let p_id = nodes.iter().position(|n| {
            n.is_element() && n.tag_name().map(|t| t.to_string() == "p").unwrap_or(false)
        });

        if let (Some(div_id), Some(p_id)) = (div_id, p_id) {
            let div_styles = styles.get(&(div_id as u32)).expect("div has styles");
            assert_eq!(div_styles.overflow_x, Overflow::Hidden);
            assert_eq!(div_styles.overflow_y, Overflow::Hidden);

            let p_styles = styles.get(&(p_id as u32)).expect("p has styles");
            // Child should have default Visible, not inherited Hidden
            assert_eq!(p_styles.overflow_x, Overflow::Visible);
            assert_eq!(p_styles.overflow_y, Overflow::Visible);
        } else {
            assert!(false, "Expected to find both <div> and <p> nodes");
        }
    }

    // ------ Inline Style Attribute Tests ------

    #[test]
    fn inline_style_applied() {
        let arena = crate::html::parse_html(r#"<div style="color: blue;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([0, 0, 255, 255])); // blue
    }

    #[test]
    fn inline_style_overrides_stylesheet() {
        let arena = crate::html::parse_html(r#"<div id="mydiv" style="color: green;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { color: red; } #mydiv { color: blue; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([0, 128, 0, 255])); // green from inline
    }

    #[test]
    fn inline_style_multiple_properties() {
        let arena = crate::html::parse_html(
            r#"<div style="color: red; font-size: 20px; background-color: yellow;">Test</div>"#,
        );
        let stylesheet = parser::parse_stylesheet("");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // red
        assert_eq!(div_styles.font_size, 20.0);
        assert_eq!(div_styles.background_color, Some([255, 255, 0, 255])); // yellow
    }

    #[test]
    fn inline_style_combines_with_stylesheet() {
        let arena = crate::html::parse_html(r#"<div style="color: red;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { font-size: 18px; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // from inline
        assert_eq!(div_styles.font_size, 18.0); // from stylesheet
    }

    #[test]
    fn inline_style_overrides_important() {
        let arena = crate::html::parse_html(r#"<div style="color: red;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { color: blue !important; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // inline wins
    }

    #[test]
    fn inline_style_no_style_attribute() {
        let arena = crate::html::parse_html("<div>No inline style</div>");
        let stylesheet = parser::parse_stylesheet("div { color: green; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        assert!(!styles.is_empty());
        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        let div_styles = styles
            .get(&(div_id.unwrap() as u32))
            .expect("div has styles");
        assert_eq!(div_styles.color, Some([0, 128, 0, 255])); // green from stylesheet
    }

    // ------ User-Agent Stylesheet Tests ------

    #[test]
    fn ua_stylesheet_has_rules() {
        let ua = user_agent_stylesheet();
        assert!(!ua.rules.is_empty(), "UA stylesheet should have rules");
    }

    #[test]
    fn ua_body_is_block_with_margin() {
        let arena = crate::html::parse_html("<html><body><p>text</p></body></html>");
        let ua = user_agent_stylesheet();
        let empty_author = parser::parse_stylesheet("");
        let merged = merge_stylesheets_with_author(&ua, &empty_author);
        let styles = compute_styles_for_tree(&arena, &merged, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let body_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "body")
                    .unwrap_or(false)
        });
        if let Some(bid) = body_id {
            let body_styles = styles.get(&(bid as u32)).expect("body has styles");
            assert_eq!(
                body_styles.display,
                DisplayType::Block,
                "body should be block"
            );
            assert_eq!(
                body_styles.margin,
                [8.0, 8.0, 8.0, 8.0],
                "body margin should be 8px"
            );
        } else {
            assert!(false, "Expected to find a <body> node");
        }
    }

    #[test]
    fn ul_has_left_padding_from_ua() {
        let arena = crate::html::parse_html("<html><body><ul><li>item</li></ul></body></html>");
        let ua = user_agent_stylesheet();
        let empty_author = parser::parse_stylesheet("");
        let merged = merge_stylesheets_with_author(&ua, &empty_author);
        let styles = compute_styles_for_tree(&arena, &merged, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let ul_id = nodes.iter().position(|n| {
            n.is_element() && n.tag_name().map(|t| t.to_string() == "ul").unwrap_or(false)
        });
        if let Some(id) = ul_id {
            let ul_styles = styles.get(&(id as u32)).expect("ul has styles");
            assert_eq!(ul_styles.display, DisplayType::Block, "ul should be block");
            assert_eq!(
                ul_styles.padding[3], 40.0,
                "ul should have left padding of 40px from UA stylesheet"
            );
        } else {
            assert!(false, "Expected to find a <ul> node");
        }
    }

    #[test]
    fn p_has_margin_from_ua() {
        let arena = crate::html::parse_html("<html><body><p>para</p></body></html>");
        let ua = user_agent_stylesheet();
        let empty_author = parser::parse_stylesheet("");
        let merged = merge_stylesheets_with_author(&ua, &empty_author);
        let styles = compute_styles_for_tree(&arena, &merged, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let p_id = nodes.iter().position(|n| {
            n.is_element() && n.tag_name().map(|t| t.to_string() == "p").unwrap_or(false)
        });
        if let Some(id) = p_id {
            let p_styles = styles.get(&(id as u32)).expect("p has styles");
            assert_eq!(p_styles.display, DisplayType::Block, "p should be block");
            // UA rule is `margin: 1em 0`, and <p> inherits the 16px default font size.
            assert_eq!(p_styles.margin[0], 16.0, "p top margin from UA (1em)");
            assert_eq!(p_styles.margin[2], 16.0, "p bottom margin from UA (1em)");
        } else {
            assert!(false, "Expected to find a <p> node");
        }
    }

    // ------ Length units ------

    /// Find the computed styles of the first element with the given tag name.
    fn styles_for_tag<'a>(
        arena: &crate::html::DomArena,
        styles: &'a FxHashMap<u32, ComputedValues>,
        tag: &str,
    ) -> &'a ComputedValues {
        let nodes = arena.nodes.borrow();
        let id = nodes
            .iter()
            .position(|n| n.is_element() && n.tag_name().is_some_and(|t| t.as_ref() == tag))
            .unwrap_or_else(|| panic!("expected a <{tag}> element"));
        styles.get(&(id as u32)).expect("element has styles")
    }

    #[test]
    fn absolute_length_units_convert_to_px() {
        let ctx = LengthContext::default();
        assert_eq!(parse_length_ctx("10px", ctx), Some(10.0));
        assert_eq!(parse_length_ctx("0", ctx), Some(0.0));
        assert_eq!(parse_length_ctx("12pt", ctx), Some(16.0));
        assert_eq!(parse_length_ctx("1in", ctx), Some(96.0));
        assert_eq!(parse_length_ctx("1pc", ctx), Some(16.0));
        assert_eq!(parse_length_ctx("2.54cm", ctx).unwrap().round(), 96.0);
        assert_eq!(parse_length_ctx("25.4mm", ctx).unwrap().round(), 96.0);
    }

    #[test]
    fn font_relative_units_use_context() {
        let ctx = LengthContext {
            font_size: 20.0,
            root_font_size: 10.0,
            ..LengthContext::default()
        };
        assert_eq!(parse_length_ctx("2em", ctx), Some(40.0));
        assert_eq!(parse_length_ctx("2rem", ctx), Some(20.0));
        assert_eq!(parse_length_ctx("1ex", ctx), Some(10.0));
    }

    #[test]
    fn viewport_units_use_context() {
        let ctx = LengthContext {
            viewport_width: 1000.0,
            viewport_height: 500.0,
            ..LengthContext::default()
        };
        assert_eq!(parse_length_ctx("50vw", ctx), Some(500.0));
        assert_eq!(parse_length_ctx("10vh", ctx), Some(50.0));
        assert_eq!(parse_length_ctx("10vmin", ctx), Some(50.0));
        assert_eq!(parse_length_ctx("10vmax", ctx), Some(100.0));
    }

    #[test]
    fn unknown_units_and_percentages_are_rejected() {
        let ctx = LengthContext::default();
        assert_eq!(parse_length_ctx("50%", ctx), None);
        assert_eq!(parse_length_ctx("10foo", ctx), None);
        assert_eq!(parse_length_ctx("auto", ctx), None);
        assert_eq!(parse_length_ctx("", ctx), None);
    }

    #[test]
    fn em_font_size_resolves_against_parent() {
        // h1's UA rule is `font-size: 2em`, so it must be twice the inherited 16px.
        let arena = crate::html::parse_html("<html><body><h1>Title</h1></body></html>");
        let ua = user_agent_stylesheet();
        let merged = merge_stylesheets_with_author(&ua, &parser::parse_stylesheet(""));
        let styles = compute_styles_for_tree(&arena, &merged, (1024.0, 768.0));

        assert_eq!(styles_for_tag(&arena, &styles, "h1").font_size, 32.0);
    }

    #[test]
    fn em_length_resolves_against_own_font_size() {
        let arena = crate::html::parse_html("<html><body><div>x</div></body></html>");
        let author = parser::parse_stylesheet("div { font-size: 20px; padding: 2em; }");
        let styles = compute_styles_for_tree(&arena, &author, (1024.0, 768.0));

        let div = styles_for_tag(&arena, &styles, "div");
        assert_eq!(div.font_size, 20.0);
        assert_eq!(div.padding, [40.0; 4], "2em against the element's own 20px");
    }

    #[test]
    fn rem_resolves_against_root_font_size() {
        let arena = crate::html::parse_html("<html><body><div>x</div></body></html>");
        let author = parser::parse_stylesheet(
            "html { font-size: 10px; } div { width: 5rem; padding: 1rem; }",
        );
        let styles = compute_styles_for_tree(&arena, &author, (1024.0, 768.0));

        let div = styles_for_tag(&arena, &styles, "div");
        assert_eq!(div.width, Some(50.0));
        assert_eq!(div.padding, [10.0; 4]);
    }

    #[test]
    fn viewport_units_resolve_against_actual_viewport() {
        let arena = crate::html::parse_html("<html><body><div>x</div></body></html>");
        let author = parser::parse_stylesheet("div { width: 50vw; height: 25vh; }");
        let styles = compute_styles_for_tree(&arena, &author, (1000.0, 800.0));

        let div = styles_for_tag(&arena, &styles, "div");
        assert_eq!(div.width, Some(500.0));
        assert_eq!(div.height, Some(200.0));
    }

    #[test]
    fn em_compounds_through_nested_elements() {
        let arena = crate::html::parse_html("<html><body><div><span>x</span></div></body></html>");
        let author =
            parser::parse_stylesheet("div { font-size: 20px; } span { font-size: 1.5em; }");
        let styles = compute_styles_for_tree(&arena, &author, (1024.0, 768.0));

        assert_eq!(styles_for_tag(&arena, &styles, "span").font_size, 30.0);
    }

    // ------ Text style flags ------

    /// Compute styles over the UA stylesheet plus the given author CSS.
    fn styles_with_ua(
        html: &str,
        author_css: &str,
    ) -> (crate::html::DomArena, FxHashMap<u32, ComputedValues>) {
        let arena = crate::html::parse_html(html);
        let merged = merge_stylesheets_with_author(
            &user_agent_stylesheet(),
            &parser::parse_stylesheet(author_css),
        );
        let styles = compute_styles_for_tree(&arena, &merged, (1024.0, 768.0));
        (arena, styles)
    }

    #[test]
    fn font_weight_keywords_and_numbers() {
        assert!(parse_is_bold("bold"));
        assert!(parse_is_bold("BOLD"));
        assert!(parse_is_bold("bolder"));
        assert!(parse_is_bold("700"));
        assert!(parse_is_bold("600"));
        assert!(!parse_is_bold("500"));
        assert!(!parse_is_bold("normal"));
        assert!(!parse_is_bold("lighter"));
    }

    #[test]
    fn ua_marks_strong_bold_and_em_italic() {
        let (arena, styles) =
            styles_with_ua("<html><body><strong>a</strong><em>b</em></body></html>", "");

        assert!(styles_for_tag(&arena, &styles, "strong").text_style.bold);
        assert!(!styles_for_tag(&arena, &styles, "strong").text_style.italic);
        assert!(styles_for_tag(&arena, &styles, "em").text_style.italic);
        assert!(!styles_for_tag(&arena, &styles, "em").text_style.bold);
    }

    #[test]
    fn ua_underlines_links_and_strikes_del() {
        let (arena, styles) = styles_with_ua(
            "<html><body><a href='#'>x</a><del>y</del></body></html>",
            "",
        );

        let a = styles_for_tag(&arena, &styles, "a");
        assert!(a.text_style.underline);
        assert!(!a.text_style.line_through);

        let del = styles_for_tag(&arena, &styles, "del");
        assert!(del.text_style.line_through);
        assert!(!del.text_style.underline);
    }

    #[test]
    fn text_decoration_parses_every_line_keyword() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>x</div></body></html>",
            "div { text-decoration: underline overline line-through; }",
        );

        let div = styles_for_tag(&arena, &styles, "div");
        assert!(div.text_style.underline);
        assert!(div.text_style.overline);
        assert!(div.text_style.line_through);
        assert!(div.text_style.has_decoration());
    }

    #[test]
    fn text_decoration_none_clears_ua_underline() {
        let (arena, styles) = styles_with_ua(
            "<html><body><a href='#'>x</a></body></html>",
            "a { text-decoration: none; }",
        );

        assert!(!styles_for_tag(&arena, &styles, "a").text_style.underline);
    }

    #[test]
    fn bold_and_italic_combine_across_nesting() {
        let (arena, styles) =
            styles_with_ua("<html><body><strong><em>x</em></strong></body></html>", "");

        let em = styles_for_tag(&arena, &styles, "em");
        assert!(em.text_style.bold, "inherits bold from <strong>");
        assert!(em.text_style.italic, "italic from its own UA rule");
    }

    #[test]
    fn font_weight_normal_overrides_inherited_bold() {
        let (arena, styles) = styles_with_ua(
            "<html><body><strong><span>x</span></strong></body></html>",
            "span { font-weight: normal; }",
        );

        assert!(!styles_for_tag(&arena, &styles, "span").text_style.bold);
    }

    #[test]
    fn merged_with_keeps_flags_set_by_either_side() {
        let child = TextStyleFlags {
            italic: true,
            ..Default::default()
        };
        let ancestor = TextStyleFlags {
            bold: true,
            underline: true,
            ..Default::default()
        };

        let merged = child.merged_with(ancestor);
        assert!(merged.bold && merged.italic && merged.underline);
        assert!(!merged.line_through && !merged.overline);
    }

    // ------ visibility / spacing / text-transform ------

    #[test]
    fn visibility_keywords_parse() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>a</div></body></html>",
            "div { visibility: hidden; }",
        );
        assert_eq!(
            styles_for_tag(&arena, &styles, "div").visibility,
            Visibility::Hidden
        );
        assert!(
            !styles_for_tag(&arena, &styles, "div")
                .visibility
                .is_painted()
        );

        let (arena, styles) = styles_with_ua(
            "<html><body><div>a</div></body></html>",
            "div { visibility: collapse; }",
        );
        assert_eq!(
            styles_for_tag(&arena, &styles, "div").visibility,
            Visibility::Collapse
        );
    }

    #[test]
    fn visibility_inherits_but_child_can_override() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div><span>a</span></div></body></html>",
            "div { visibility: hidden; }",
        );
        assert_eq!(
            styles_for_tag(&arena, &styles, "span").visibility,
            Visibility::Hidden,
            "inherits hidden from the parent"
        );

        let (arena, styles) = styles_with_ua(
            "<html><body><div><span>a</span></div></body></html>",
            "div { visibility: hidden; } span { visibility: visible; }",
        );
        assert_eq!(
            styles_for_tag(&arena, &styles, "span").visibility,
            Visibility::Visible,
            "a child may re-show itself inside a hidden parent"
        );
    }

    #[test]
    fn letter_and_word_spacing_parse_lengths() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>a</div></body></html>",
            "div { font-size: 10px; letter-spacing: 0.2em; word-spacing: 4px; }",
        );

        let div = styles_for_tag(&arena, &styles, "div");
        assert_eq!(div.text_style.letter_spacing, 2.0, "0.2em of 10px");
        assert_eq!(div.text_style.word_spacing, 4.0);
    }

    #[test]
    fn spacing_normal_is_zero() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>a</div></body></html>",
            "div { letter-spacing: normal; word-spacing: normal; }",
        );

        let div = styles_for_tag(&arena, &styles, "div");
        assert_eq!(div.text_style.letter_spacing, 0.0);
        assert_eq!(div.text_style.word_spacing, 0.0);
    }

    #[test]
    fn text_transform_parses_and_inherits() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div><span>a</span></div></body></html>",
            "div { text-transform: uppercase; }",
        );

        assert_eq!(
            styles_for_tag(&arena, &styles, "div").text_style.transform,
            TextTransform::Uppercase
        );
        assert_eq!(
            styles_for_tag(&arena, &styles, "span").text_style.transform,
            TextTransform::Uppercase,
            "text-transform inherits"
        );
    }

    #[test]
    fn text_transform_applies_to_text() {
        assert_eq!(TextTransform::None.apply("hello world"), "hello world");
        assert_eq!(TextTransform::Uppercase.apply("hello world"), "HELLO WORLD");
        assert_eq!(TextTransform::Lowercase.apply("HELLO World"), "hello world");
        assert_eq!(
            TextTransform::Capitalize.apply("hello world"),
            "Hello World"
        );
    }

    #[test]
    fn capitalize_leaves_the_rest_of_each_word_alone() {
        // Per CSS, capitalize only touches the first letter of each word.
        assert_eq!(
            TextTransform::Capitalize.apply("hELLO wORLD"),
            "HELLO WORLD"
        );
        assert_eq!(
            TextTransform::Capitalize.apply("  spaced  out "),
            "  Spaced  Out "
        );
        assert_eq!(TextTransform::Capitalize.apply(""), "");
    }

    #[test]
    fn text_transform_handles_non_ascii() {
        assert_eq!(TextTransform::Uppercase.apply("straße"), "STRASSE");
        assert_eq!(
            TextTransform::Capitalize.apply("études du soir"),
            "Études Du Soir"
        );
        // Japanese has no case, so it must pass through untouched.
        assert_eq!(TextTransform::Uppercase.apply("日本語"), "日本語");
    }

    #[test]
    fn author_style_overrides_ua() {
        let arena = crate::html::parse_html("<html><body><div>test</div></body></html>");
        let ua = user_agent_stylesheet();
        let author = parser::parse_stylesheet("div { display: inline; }");
        let merged = merge_stylesheets_with_author(&ua, &author);
        let styles = compute_styles_for_tree(&arena, &merged, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        if let Some(id) = div_id {
            let div_styles = styles.get(&(id as u32)).expect("div has styles");
            assert_eq!(
                div_styles.display,
                DisplayType::Inline,
                "Author style should override UA stylesheet"
            );
        } else {
            assert!(false, "Expected to find a <div> node");
        }
    }

    #[test]
    fn test_parse_flex_basis_percentage_50() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "flex-basis".to_string(),
            value: "50%".to_string(),
            important: false,
        });
        assert_eq!(computed.flex_basis, FlexBasis::Percentage(0.5));
    }

    #[test]
    fn test_parse_box_sizing_border_box() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "box-sizing".to_string(),
            value: "border-box".to_string(),
            important: false,
        });
        assert_eq!(computed.box_sizing, BoxSizing::BorderBox);
    }

    // ------ Grid Property Parsing Tests ------

    #[test]
    fn test_parse_display_grid() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "display".to_string(),
            value: "grid".to_string(),
            important: false,
        });
        assert_eq!(computed.display, DisplayType::Grid);
    }

    #[test]
    fn test_parse_grid_track_list() {
        // Test parsing "1fr 1fr 200px auto"
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "grid-template-columns".to_string(),
            value: "1fr 2fr 200px auto".to_string(),
            important: false,
        });
        assert_eq!(computed.grid_template_columns.len(), 4);
        assert_eq!(computed.grid_template_columns[0], GridTrack::Fr(1.0));
        assert_eq!(computed.grid_template_columns[1], GridTrack::Fr(2.0));
        assert_eq!(computed.grid_template_columns[2], GridTrack::Fixed(200.0));
        assert_eq!(computed.grid_template_columns[3], GridTrack::Auto);
    }

    #[test]
    fn test_parse_grid_track_list_rows() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "grid-template-rows".to_string(),
            value: "100px 1fr auto".to_string(),
            important: false,
        });
        assert_eq!(computed.grid_template_rows.len(), 3);
        assert_eq!(computed.grid_template_rows[0], GridTrack::Fixed(100.0));
        assert_eq!(computed.grid_template_rows[1], GridTrack::Fr(1.0));
        assert_eq!(computed.grid_template_rows[2], GridTrack::Auto);
    }

    #[test]
    fn test_parse_grid_column_gap() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "grid-column-gap".to_string(),
            value: "16px".to_string(),
            important: false,
        });
        assert_eq!(computed.grid_column_gap, 16.0);
    }

    #[test]
    fn test_parse_grid_row_gap() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "grid-row-gap".to_string(),
            value: "8px".to_string(),
            important: false,
        });
        assert_eq!(computed.grid_row_gap, 8.0);
    }

    #[test]
    fn test_parse_grid_defaults() {
        let computed = ComputedValues::default();
        assert!(computed.grid_template_columns.is_empty());
        assert!(computed.grid_template_rows.is_empty());
        assert_eq!(computed.grid_column_gap, 0.0);
        assert_eq!(computed.grid_row_gap, 0.0);
    }

    #[test]
    fn test_parse_order() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "order".to_string(),
            value: "5".to_string(),
            important: false,
        });
        assert_eq!(computed.order, 5);

        let computed2 = ComputedValues::default().from_declaration(&Declaration {
            property: "order".to_string(),
            value: "-2".to_string(),
            important: false,
        });
        assert_eq!(computed2.order, -2);
    }

    #[test]
    fn test_order_default() {
        let computed = ComputedValues::default();
        assert_eq!(computed.order, 0);
    }

    // -- Hover style computation tests --

    #[test]
    fn test_hover_style_applied_when_hovered() {
        use crate::html;
        // Simple page with an <a> tag and :hover CSS rule
        let html = r##"<html><body><a href="#">Link</a></body></html>"##;
        let css_str = "a:hover { color: red }";

        let arena = html::parse_html(html);
        let author_stylesheet = parser::parse_stylesheet(css_str);
        let ua_stylesheet = user_agent_stylesheet();
        let stylesheet = merge_stylesheets_with_author(&ua_stylesheet, &author_stylesheet);

        // Without hover — the <a> should NOT have red color from :hover rule
        let styles_no_hover = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        // With hover — mark node_id 3 (the <a>) as hovered
        // Node IDs: 0=document, 1=html, 2=body, 3=a
        let nodes_ref = arena.nodes.borrow();
        // Find the <a> element node ID by scanning
        let link_node_id = nodes_ref
            .iter()
            .enumerate()
            .find(|(_, n)| n.is_element() && n.tag_name().map(|t| t.as_ref()) == Some("a"))
            .map(|(i, _)| i as u32);
        drop(nodes_ref);

        // Compute with the <a> marked as hovered
        let link_id = link_node_id.expect("Should have found <a> tag");
        let styles_with_hover =
            compute_styles_for_tree_with_hover(&arena, &stylesheet, (1024.0, 768.0), &[link_id]);

        // The hovered node should have styles computed from the :hover rule
        // Check that it has a non-default color
        if let Some(computed) = styles_with_hover.get(&link_id) {
            assert_eq!(
                computed.color,
                Some([255, 0, 0, 255]),
                "Hovered <a> should have red color"
            );
        } else {
            panic!("Node {} (<a>) should have computed styles", link_id);
        }

        // Without hover, the color should NOT be red (no other rule sets it)
        if let Some(computed) = styles_no_hover.get(&link_id) {
            assert_ne!(
                computed.color,
                Some([255, 0, 0, 255]),
                "Non-hovered <a> should NOT have red color"
            );
        }
    }

    #[test]
    fn test_static_compute_same_as_hover_with_empty_set() {
        use crate::html;
        let html = r#"<html><body><div>Hello</div></body></html>"#;
        let css_str = "div:hover { background-color: blue }";

        let arena = html::parse_html(html);
        let author_stylesheet = parser::parse_stylesheet(css_str);
        let ua_stylesheet = user_agent_stylesheet();
        let stylesheet = merge_stylesheets_with_author(&ua_stylesheet, &author_stylesheet);

        // Static computation should produce same result as hover with empty set
        let static_styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));
        let no_hover_styles =
            compute_styles_for_tree_with_hover(&arena, &stylesheet, (1024.0, 768.0), &[]);

        // Both should have the same number of entries and same background_color for each node
        assert_eq!(static_styles.len(), no_hover_styles.len());
        for (&id, computed) in &static_styles {
            if let Some(other) = no_hover_styles.get(&id) {
                assert_eq!(
                    computed.background_color, other.background_color,
                    "Node {} background_color mismatch",
                    id
                );
                assert_eq!(computed.color, other.color, "Node {} color mismatch", id);
            }
        }
    }

    #[test]
    fn test_table_display_types() {
        let decls = [
            ("display: table", DisplayType::Table),
            ("display: inline-table", DisplayType::InlineTable),
            ("display: table-row-group", DisplayType::TableRowGroup),
            ("display: table-header-group", DisplayType::TableHeaderGroup),
            ("display: table-footer-group", DisplayType::TableFooterGroup),
            ("display: table-row", DisplayType::TableRow),
            ("display: table-cell", DisplayType::TableCell),
            ("display: table-caption", DisplayType::TableCaption),
            ("display: table-column", DisplayType::TableColumn),
            ("display: table-column-group", DisplayType::TableColumnGroup),
        ];

        for (css_str, expected) in decls {
            let parsed = parse_declarations(css_str);
            let mut computed = ComputedValues::default();
            for decl in parsed {
                computed = computed.from_declaration(&decl);
            }
            assert_eq!(computed.display, expected, "Failed for {}", css_str);
        }
    }

    #[test]
    fn test_grid_track_advanced_parsing() {
        let tracks = parse_grid_track_list(
            "min-content 1fr max-content 200px fit-content(300px) auto",
            LengthContext::default(),
        );
        assert_eq!(tracks.len(), 6);
        assert_eq!(tracks[0], GridTrack::MinContent);
        assert_eq!(tracks[1], GridTrack::Fr(1.0));
        assert_eq!(tracks[2], GridTrack::MaxContent);
        assert_eq!(tracks[3], GridTrack::Fixed(200.0));
        assert_eq!(tracks[4], GridTrack::FitContent(300.0));
        assert_eq!(tracks[5], GridTrack::Auto);
    }

    #[test]
    fn test_grid_repeat_syntax() {
        let tracks = parse_grid_track_list(
            "repeat(3, 1fr) 200px repeat(2, min-content 100px)",
            LengthContext::default(),
        );
        assert_eq!(tracks.len(), 8);
        assert_eq!(tracks[0], GridTrack::Fr(1.0));
        assert_eq!(tracks[1], GridTrack::Fr(1.0));
        assert_eq!(tracks[2], GridTrack::Fr(1.0));
        assert_eq!(tracks[3], GridTrack::Fixed(200.0));
        assert_eq!(tracks[4], GridTrack::MinContent);
        assert_eq!(tracks[5], GridTrack::Fixed(100.0));
        assert_eq!(tracks[6], GridTrack::MinContent);
        assert_eq!(tracks[7], GridTrack::Fixed(100.0));
    }

    #[test]
    fn test_align_self_parsing() {
        let decls = [
            ("align-self: auto", AlignSelf::Auto),
            ("align-self: flex-start", AlignSelf::FlexStart),
            ("align-self: flex-end", AlignSelf::FlexEnd),
            ("align-self: center", AlignSelf::Center),
            ("align-self: stretch", AlignSelf::Stretch),
        ];

        for (css_str, expected) in decls {
            let parsed = parse_declarations(css_str);
            let mut computed = ComputedValues::default();
            for decl in parsed {
                computed = computed.from_declaration(&decl);
            }
            assert_eq!(computed.align_self, expected, "Failed for {}", css_str);
        }
    }

    #[test]
    fn test_css_variable_resolution_basic() {
        let mut vars = rustc_hash::FxHashMap::default();
        vars.insert("--main-color".to_string(), "#ff0000".to_string());
        vars.insert("--base-size".to_string(), "20px".to_string());

        let res1 = resolve_var_functions("var(--main-color)", &vars);
        assert_eq!(res1, "#ff0000");

        let res2 = resolve_var_functions("var(--base-size)", &vars);
        assert_eq!(res2, "20px");

        let res3 = resolve_var_functions("var(--missing, #00ff00)", &vars);
        assert_eq!(res3, "#00ff00");

        let res4 = resolve_var_functions("var(--missing, var(--main-color))", &vars);
        assert_eq!(res4, "#ff0000");
    }

    #[test]
    fn test_css_variable_inheritance_in_tree() {
        let html = r#"
            <html>
                <body>
                    <div id="parent">
                        <span id="child">Text</span>
                    </div>
                </body>
            </html>
        "#;
        let css = r#"
            #parent {
                --theme-color: rgb(255, 128, 0);
                color: var(--theme-color);
            }
            #child {
                background-color: var(--theme-color);
            }
        "#;
        let arena = crate::html::parse_html(html);
        let stylesheet = parser::parse_stylesheet(css);
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let child_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("child"))
            .unwrap() as u32;

        let child_styles = styles.get(&child_id).expect("child styles");
        assert_eq!(child_styles.background_color, Some([255, 128, 0, 255]));
        assert_eq!(child_styles.color, Some([255, 128, 0, 255]));
    }

    #[test]
    fn test_attribute_selectors_comprehensive() {
        let html = r#"
            <html>
                <body>
                    <a id="a1" href="https://example.com/page.html" class="btn primary" target="_blank">Link 1</a>
                    <a id="a2" href="/relative/path" class="btn secondary">Link 2</a>
                    <div id="d1" data-role="custom-widget-v1">Widget</div>
                </body>
            </html>
        "#;
        let css = r#"
            a[target] { color: rgb(1, 1, 1); }
            a[href^="https://"] { font-size: 24px; }
            a[href$=".html"] { line-height: 2.0; }
            a[class~="primary"] { margin-top: 15px; }
            div[data-role*="widget"] { padding-left: 30px; }
        "#;
        let arena = crate::html::parse_html(html);
        let stylesheet = parser::parse_stylesheet(css);
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let a1_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("a1"))
            .unwrap() as u32;
        let a2_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("a2"))
            .unwrap() as u32;
        let d1_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("d1"))
            .unwrap() as u32;

        let a1_styles = styles.get(&a1_id).unwrap();
        assert_eq!(a1_styles.color, Some([1, 1, 1, 255]), "a1 matches [target]");
        assert_eq!(a1_styles.font_size, 24.0, "a1 matches [href^='https://']");
        assert_eq!(a1_styles.line_height, 2.0, "a1 matches [href$='.html']");
        assert_eq!(a1_styles.margin[0], 15.0, "a1 matches [class~='primary']");

        let a2_styles = styles.get(&a2_id).unwrap();
        assert_ne!(
            a2_styles.color,
            Some([1, 1, 1, 255]),
            "a2 should NOT match [target]"
        );

        let d1_styles = styles.get(&d1_id).unwrap();
        assert_eq!(
            d1_styles.padding[3], 30.0,
            "d1 matches [data-role*='widget']"
        );
    }

    #[test]
    fn test_pseudo_classes_nth_child_last_child() {
        let html = r#"
            <html>
                <body>
                    <ul id="list">
                        <li id="li1">Item 1</li>
                        <li id="li2">Item 2</li>
                        <li id="li3">Item 3</li>
                        <li id="li4">Item 4</li>
                    </ul>
                </body>
            </html>
        "#;
        let css = r#"
            li:first-child { margin-top: 10px; }
            li:last-child { margin-bottom: 20px; }
            li:nth-child(even) { font-size: 22px; }
        "#;
        let arena = crate::html::parse_html(html);
        let stylesheet = parser::parse_stylesheet(css);
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let li1_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("li1"))
            .unwrap() as u32;
        let li2_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("li2"))
            .unwrap() as u32;
        let li4_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("li4"))
            .unwrap() as u32;

        let li1 = styles.get(&li1_id).unwrap();
        assert_eq!(li1.margin[0], 10.0, "li1 is :first-child");

        let li2 = styles.get(&li2_id).unwrap();
        assert_eq!(li2.font_size, 22.0, "li2 is 2nd child (:nth-child(even))");

        let li4 = styles.get(&li4_id).unwrap();
        assert_eq!(li4.margin[2], 20.0, "li4 is :last-child");
        assert_eq!(li4.font_size, 22.0, "li4 is 4th child (:nth-child(even))");
    }
}
