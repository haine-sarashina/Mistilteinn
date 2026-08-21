pub mod animation;
pub mod filter;
pub mod parser;

pub use animation::{Animation, AnimationDirection, Easing, FillMode, KeyframesRule, Transitions};
pub use filter::{Filter, FilterFn};

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
#[derive(Debug, Clone, PartialEq)]
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
///
/// `initial_font_size` is the size an element has when nothing set one, which
/// page zoom moves away from 16px — it is the "nobody assigned this" marker,
/// not a constant.
fn inherit_properties(
    parent: &ComputedValues,
    mut child: ComputedValues,
    initial_font_size: f32,
) -> ComputedValues {
    // `color` inherits
    if parent.color.is_some() && child.color.is_none() {
        child.color = parent.color;
    }
    // `font-size` inherits, and is also the base a percentage or `em` on the
    // child resolves against.
    child.inherited_font_size = parent.font_size;
    if child.font_size == initial_font_size && parent.font_size != initial_font_size {
        child.font_size = parent.font_size;
    }
    // The `list-style` properties inherit, which is how `ul { list-style-type:
    // disc }` reaches the items rather than the list box that is never marked.
    if child.list_style_type.is_none() {
        child.list_style_type = parent.list_style_type;
    }
    if child.list_style_position.is_none() {
        child.list_style_position = parent.list_style_position;
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
    if child.text_align == TextAlign::Start && parent.text_align != TextAlign::Start {
        child.text_align = parent.text_align;
    }
    // `direction` inherits.
    if child.direction == Direction::Ltr && parent.direction != Direction::Ltr {
        child.direction = parent.direction;
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
    // `cursor` inherits; `Auto` is the initial, so it is also "unset here".
    if child.cursor == Cursor::Auto {
        child.cursor = parent.cursor;
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

    let mut size = DEFAULT_FONT_SIZE * ctx.zoom;
    let apply = |value: &str, size: &mut f32| {
        let local = LengthContext {
            font_size: *size,
            root_font_size: *size,
            ..ctx
        };
        // A percentage on the root resolves against the initial font size.
        if let Some(pct) = value.trim().strip_suffix('%') {
            if let Ok(n) = pct.trim().parse::<f32>() {
                *size = DEFAULT_FONT_SIZE * ctx.zoom * n / 100.0;
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

/// The values an element starts from before any declaration applies.
///
/// The CSS initial values, except that the initial font size is scaled by the
/// page zoom: text nobody sized explicitly has to grow with everything else.
pub fn initial_values(zoom: f32) -> ComputedValues {
    ComputedValues {
        font_size: DEFAULT_FONT_SIZE * zoom,
        inherited_font_size: DEFAULT_FONT_SIZE * zoom,
        ..ComputedValues::default()
    }
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
    compute_styles_for_tree_internal(arena, stylesheet, viewport, 1.0, |_| false, |_| false)
}

/// Compute styles with both a hover context and a page zoom factor.
///
/// Zoom multiplies every absolute length, so the page reflows into the window
/// it already has rather than being scaled up as an image would be.
pub fn compute_styles_for_tree_with_hover_zoom(
    arena: &crate::html::DomArena,
    stylesheet: &parser::Stylesheet,
    viewport: (f32, f32),
    hovered_ids: &[u32],
    zoom: f32,
) -> FxHashMap<u32, ComputedValues> {
    compute_styles_for_tree_with_state(
        arena,
        stylesheet,
        viewport,
        &InteractionState {
            hovered: hovered_ids,
            focused: None,
        },
        zoom,
    )
}

/// What the pointer and the keyboard are doing to the page right now.
///
/// The dynamic half of the cascade. Everything else a selector asks about can
/// be read off the document; these two change while the document stands still,
/// and a style recompute is how the page finds out.
#[derive(Debug, Default, Clone, Copy)]
pub struct InteractionState<'a> {
    /// Elements under the cursor, ancestors included, so that `div:hover > a`
    /// can be answered from the element the rule is about.
    pub hovered: &'a [u32],
    /// The element holding keyboard focus, if there is one.
    pub focused: Option<u32>,
}

/// Compute styles against the page's current pointer and keyboard state.
pub fn compute_styles_for_tree_with_state(
    arena: &crate::html::DomArena,
    stylesheet: &parser::Stylesheet,
    viewport: (f32, f32),
    state: &InteractionState<'_>,
    zoom: f32,
) -> FxHashMap<u32, ComputedValues> {
    let hover_set: std::collections::HashSet<u32> = state.hovered.iter().copied().collect();
    let focused = state.focused;
    compute_styles_for_tree_internal(
        arena,
        stylesheet,
        viewport,
        zoom,
        |id| hover_set.contains(&id),
        move |id| focused == Some(id),
    )
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
    compute_styles_for_tree_with_hover_zoom(arena, stylesheet, viewport, hovered_ids, 1.0)
}

/// Internal implementation of style computation with the dynamic state as
/// predicates.
///
/// `is_hovered` and `is_focused` answer `:hover` and `:focus` for one element.
/// Pass `|_id| false` for a static computation.
fn compute_styles_for_tree_internal<F, G>(
    arena: &crate::html::DomArena,
    stylesheet: &parser::Stylesheet,
    viewport: (f32, f32),
    zoom: f32,
    is_hovered: F,
    is_focused: G,
) -> FxHashMap<u32, ComputedValues>
where
    F: Fn(u32) -> bool,
    G: Fn(u32) -> bool,
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

    let index = DomIndex::build(arena, &nodes_ref, &element_ids);

    drop(nodes_ref);

    // Phase 1: Cascade — for each element, find matching rules and apply
    let start = std::time::Instant::now();
    let total_elements = element_ids.len();

    // Collect active rules from the stylesheet based on viewport width
    let mut active_rules: Vec<&parser::CSSRule> = stylesheet.rules.iter().collect();
    for media_rule in &stylesheet.media_rules {
        if parser::evaluate_media_condition(
            &media_rule.condition,
            parser::MediaContext::new(viewport.0, viewport.1),
        ) {
            active_rules.extend(media_rule.rules.iter());
        }
    }
    // Back into document order. The declarations are applied in this order at
    // equal specificity, so a `@media` rule must not jump ahead of a later
    // unconditional one simply for being conditional.
    active_rules.sort_by_key(|rule| rule.order);

    // Keep applied declarations per element so we can re-evaluate variables after inheriting from parent
    let mut applied_decls_per_element: rustc_hash::FxHashMap<u32, Vec<Declaration>> =
        rustc_hash::FxHashMap::default();

    // Base context for resolving relative lengths. `rem` needs the root font
    // size, which is whatever `<html>` resolves to, so compute that first.
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    let mut length_ctx = LengthContext {
        // The initial font size is a length like any other, so zoom applies to
        // it too — otherwise a zoomed page grows everything except its text.
        font_size: DEFAULT_FONT_SIZE * zoom,
        root_font_size: DEFAULT_FONT_SIZE * zoom,
        viewport_width: viewport.0,
        viewport_height: viewport.1,
        zoom,
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
            matches_for(id, sel, arena, &index, &is_hovered, &is_focused)
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

        let mut computed = initial_values(zoom);

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
                let mut inherited =
                    inherit_properties(&parent_styles, initial_values(zoom), length_ctx.font_size);

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

    // Phase 3: the boxes `::before` and `::after` generate.
    //
    // These are done last because a generated box inherits from the element it
    // hangs off, and that element's own style is only final after Phase 2.
    // They are stored in the same map under a key derived from their element,
    // so everything that carries styles around keeps carrying one map.
    for &node_id in &element_ids {
        for kind in PseudoKind::all() {
            let mut matched: Vec<(&Declaration, (u32, u32, u32))> = Vec::new();

            for rule in &active_rules {
                for selector in &rule.selectors {
                    if !selector
                        .pseudo_element()
                        .is_some_and(|name| name.eq_ignore_ascii_case(kind.name()))
                    {
                        continue;
                    }
                    let element_part = selector.without_pseudo_element();
                    let simple_match = |id: u32, sel: &parser::SimpleSelector| -> bool {
                        matches_for(id, sel, arena, &index, &is_hovered, &is_focused)
                    };
                    let get_parent = |id: u32| -> Option<u32> {
                        arena
                            .get(crate::html::DomHandle(crate::html::NodeId::from_raw(id)))
                            .and_then(|n| n.parent_id())
                    };
                    if element_part.full_matches(node_id, &get_parent, &simple_match) {
                        let spec = selector.specificity();
                        matched.extend(rule.declarations.iter().map(|decl| (decl, spec)));
                    }
                }
            }
            if matched.is_empty() {
                continue;
            }

            // Normal declarations first, then `!important`, each in ascending
            // specificity — the same order the element cascade uses.
            matched.sort_by_key(|(decl, spec)| (decl.important, *spec));

            let origin = result.get(&node_id).cloned().unwrap_or_default();
            let mut computed =
                inherit_properties(&origin, initial_values(zoom), length_ctx.font_size);
            // A generated box is inline unless the page says otherwise.
            computed.display = DisplayType::Inline;
            for (decl, _) in matched {
                computed = computed.from_declaration_with_ctx(decl, length_ctx);
            }

            if computed.content.generates_box() {
                result.insert(pseudo_style_key(node_id, kind), computed);
            }
        }
    }

    result
}

/// Everything about the shape of the document that the selectors ask about.
///
/// Worked out once for the whole tree rather than per element per selector: a
/// page has a few hundred distinct selectors and thousands of elements, and
/// every one of `:first-child`, `:nth-of-type` and `:empty` is a question about
/// where an element sits rather than about the rule asking.
#[derive(Default)]
struct DomIndex {
    classes: rustc_hash::FxHashMap<u32, Vec<String>>,
    ids: rustc_hash::FxHashMap<u32, String>,
    /// Position among element siblings: `(from the start, from the end)`,
    /// 1-based. An element missing from the map has no known position.
    child_index: rustc_hash::FxHashMap<u32, (usize, usize)>,
    /// The same, counting only siblings that share a tag name.
    type_index: rustc_hash::FxHashMap<u32, (usize, usize)>,
    /// Elements with nothing inside them, for `:empty`.
    empty: std::collections::HashSet<u32>,
    /// The element the document hangs off, for `:root`.
    root: Option<u32>,
}

impl DomIndex {
    fn build(
        arena: &crate::html::DomArena,
        nodes: &[crate::html::DomNode],
        element_ids: &[u32],
    ) -> Self {
        let mut index = DomIndex {
            root: element_ids.first().copied(),
            ..Default::default()
        };

        // A set, not a scan of the list: this is asked once per child of every
        // node in the document, and the list is as long as the document.
        let elements: std::collections::HashSet<u32> = element_ids.iter().copied().collect();

        for node in nodes {
            let siblings: Vec<u32> = node
                .children
                .iter()
                .map(|h| h.index() as u32)
                .filter(|id| elements.contains(id))
                .collect();
            let total = siblings.len();

            // Of-type positions are counted per tag name within this parent.
            let mut total_of_type: rustc_hash::FxHashMap<String, usize> =
                rustc_hash::FxHashMap::default();
            for &id in &siblings {
                if let Some(tag) = tag_of(arena, id) {
                    *total_of_type.entry(tag).or_insert(0) += 1;
                }
            }

            let mut seen_of_type: rustc_hash::FxHashMap<String, usize> =
                rustc_hash::FxHashMap::default();
            for (i, &id) in siblings.iter().enumerate() {
                index.child_index.insert(id, (i + 1, total - i));
                if let Some(tag) = tag_of(arena, id) {
                    let seen = seen_of_type.entry(tag.clone()).or_insert(0);
                    *seen += 1;
                    let of_type_total = total_of_type.get(&tag).copied().unwrap_or(*seen);
                    index
                        .type_index
                        .insert(id, (*seen, of_type_total + 1 - *seen));
                }
            }
        }

        for &id in element_ids {
            let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
            let Some(node) = arena.get(handle) else {
                continue;
            };
            if let Some(attr) = node.get_attr("class") {
                index
                    .classes
                    .insert(id, attr.split_whitespace().map(String::from).collect());
            }
            if let Some(id_attr) = node.get_attr("id") {
                index.ids.insert(id, id_attr.to_string());
            }
            if is_empty_element(arena, id) {
                index.empty.insert(id);
            }
        }

        index
    }
}

/// One element's tag name.
fn tag_of(arena: &crate::html::DomArena, id: u32) -> Option<String> {
    arena
        .get(crate::html::DomHandle(crate::html::NodeId::from_raw(id)))
        .and_then(|n| n.tag_name().map(|t| t.to_string()))
}

/// Whether an element has nothing in it, for `:empty`.
///
/// Whitespace between tags does not count as content. That is the Selectors 4
/// reading and what browsers do; taking the older reading would mean a page
/// laid out with newlines between its tags has no empty elements at all.
fn is_empty_element(arena: &crate::html::DomArena, id: u32) -> bool {
    let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
    let Some(node) = arena.get(handle) else {
        return false;
    };
    node.children.iter().all(|child| {
        let child_handle =
            crate::html::DomHandle(crate::html::NodeId::from_raw(child.index() as u32));
        match arena.get(child_handle) {
            Some(child_node) => child_node
                .text_content()
                .is_some_and(|text| text.trim().is_empty()),
            None => true,
        }
    })
}

/// Match one simple selector against one element.
///
/// Both cascade passes — elements and their `::before` / `::after` boxes — ask
/// the same question, so the facts are assembled in one place.
fn matches_for(
    id: u32,
    sel: &parser::SimpleSelector,
    arena: &crate::html::DomArena,
    index: &DomIndex,
    is_hovered: &impl Fn(u32) -> bool,
    is_focused: &impl Fn(u32) -> bool,
) -> bool {
    let Some(node) = arena.get(crate::html::DomHandle(crate::html::NodeId::from_raw(id))) else {
        return false;
    };
    let Some(tag) = node.tag_name() else {
        return false;
    };
    let tag: &str = tag;

    // A control is checked or disabled if it says so in the markup. Nothing
    // toggles a control yet, so the attribute is the whole story.
    let checked = || match tag {
        "option" => node.get_attr("selected").is_some(),
        _ => node.get_attr("checked").is_some(),
    };

    parser::Selector::simple_matches_facts(
        sel,
        &parser::ElementFacts {
            classes: &|c| {
                index
                    .classes
                    .get(&id)
                    .is_some_and(|classes| classes.iter().any(|cls| cls == c))
            },
            has_id: &|i| index.ids.get(&id).is_some_and(|id_str| id_str == i),
            matches_attr: &|name, op, val| match node.get_attr(name) {
                Some(attr_val) => parser::evaluate_attr_operator(attr_val, op, val),
                None => false,
            },
            child_index: &|| index.child_index.get(&id).copied().unwrap_or((0, 0)),
            type_index: &|| index.type_index.get(&id).copied().unwrap_or((0, 0)),
            is_root: &|| index.root == Some(id),
            is_empty: &|| index.empty.contains(&id),
            is_hovered: &|| is_hovered(id),
            is_focused: &|| is_focused(id),
            is_checked: &checked,
            is_disabled: &|| node.get_attr("disabled").is_some(),
            ..parser::ElementFacts::for_tag(tag)
        },
    )
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

/// The computed `list-style-type` CSS property: what a list item is marked
/// with.
///
/// The set a page actually reaches for. A keyword we do not know falls back to
/// `Disc` rather than to nothing, because a list with no markers at all reads
/// as a stack of paragraphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListStyleType {
    None,
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

impl ListStyleType {
    fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "disc" => Self::Disc,
            "circle" => Self::Circle,
            "square" => Self::Square,
            "decimal" => Self::Decimal,
            "decimal-leading-zero" => Self::DecimalLeadingZero,
            "lower-alpha" | "lower-latin" => Self::LowerAlpha,
            "upper-alpha" | "upper-latin" => Self::UpperAlpha,
            "lower-roman" => Self::LowerRoman,
            "upper-roman" => Self::UpperRoman,
            _ => return None,
        })
    }

    /// The text drawn beside the item that is `ordinal` in its list.
    ///
    /// `None` where there is no marker to draw. The trailing dot on a number is
    /// part of the marker, which is why this returns the whole string rather
    /// than a glyph the caller has to dress.
    pub fn marker_text(self, ordinal: i32) -> Option<String> {
        Some(match self {
            Self::None => return None,
            Self::Disc => "\u{2022}".to_string(),
            Self::Circle => "\u{25e6}".to_string(),
            Self::Square => "\u{25aa}".to_string(),
            Self::Decimal => format!("{ordinal}."),
            Self::DecimalLeadingZero => {
                if (0..10).contains(&ordinal) {
                    format!("0{ordinal}.")
                } else {
                    format!("{ordinal}.")
                }
            }
            Self::LowerAlpha => format!("{}.", alphabetic(ordinal, 'a')),
            Self::UpperAlpha => format!("{}.", alphabetic(ordinal, 'A')),
            Self::LowerRoman => format!("{}.", roman(ordinal).to_lowercase()),
            Self::UpperRoman => format!("{}.", roman(ordinal)),
        })
    }
}

/// The bijective base-26 counter: a, b, … z, aa, ab, …
///
/// Out of range — zero or negative, which `start` and `value` can both ask for
/// — falls back to the number, which is what browsers do rather than printing
/// nothing.
fn alphabetic(ordinal: i32, first: char) -> String {
    if ordinal < 1 {
        return ordinal.to_string();
    }
    let mut n = ordinal as u32;
    let mut out = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push((first as u8 + rem as u8) as char);
        n = (n - 1) / 26;
    }
    out.iter().rev().collect()
}

/// Roman numerals, falling back to the number outside the range they cover.
fn roman(ordinal: i32) -> String {
    const NUMERALS: [(i32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    if !(1..4000).contains(&ordinal) {
        return ordinal.to_string();
    }
    let mut left = ordinal;
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while left >= value {
            out.push_str(numeral);
            left -= value;
        }
    }
    out
}

/// The computed `list-style-position` CSS property: whether the marker sits in
/// the item's content or beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListStylePosition {
    /// Beside the item, in the space the list's padding leaves for it. The
    /// item's own text starts at the same place on every line.
    #[default]
    Outside,
    /// The first thing on the item's first line, pushing the text along.
    Inside,
}

impl ListStylePosition {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "outside" => Some(Self::Outside),
            "inside" => Some(Self::Inside),
            _ => None,
        }
    }
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
    /// In flow like `relative`, but offset by however far its scroll container
    /// has scrolled past it, and never further than its containing block.
    Sticky,
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

// ------ Backgrounds ------

/// A `background-size` component, or a `background-position` coordinate.
///
/// Percentages cannot be resolved during the cascade — they depend on the box
/// and, for `background-position`, on the image — so they are kept symbolic
/// until paint time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundLength {
    /// `auto` for a size, or the corresponding edge keyword for a position.
    Auto,
    Pixels(f32),
    /// A fraction, where `1.0` is 100%.
    Percent(f32),
}

impl BackgroundLength {
    fn parse(s: &str, ctx: LengthContext) -> Option<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        if let Some(pct) = s.strip_suffix('%') {
            return pct
                .trim()
                .parse::<f32>()
                .ok()
                .map(|p| Self::Percent(p / 100.0));
        }
        parse_length_ctx(s, ctx).map(Self::Pixels)
    }
}

/// The computed `background-size`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BackgroundSize {
    /// Draw at the image's own pixel size.
    #[default]
    Auto,
    /// Scale, preserving aspect ratio, until the box is fully covered.
    Cover,
    /// Scale, preserving aspect ratio, until the image fits inside the box.
    Contain,
    /// Explicit width and height; `Auto` on one axis keeps the aspect ratio.
    Explicit(BackgroundLength, BackgroundLength),
}

/// The computed `background-repeat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundRepeat {
    #[default]
    Repeat,
    RepeatX,
    RepeatY,
    NoRepeat,
}

impl BackgroundRepeat {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "repeat" => Self::Repeat,
            "repeat-x" => Self::RepeatX,
            "repeat-y" => Self::RepeatY,
            "no-repeat" => Self::NoRepeat,
            // `space` and `round` change tile spacing rather than whether the
            // image tiles; plain repeat is the closest of the four.
            "space" | "round" => Self::Repeat,
            _ => return None,
        })
    }

    /// Whether tiles are laid out along each axis: (horizontal, vertical).
    pub fn axes(self) -> (bool, bool) {
        match self {
            Self::Repeat => (true, true),
            Self::RepeatX => (true, false),
            Self::RepeatY => (false, true),
            Self::NoRepeat => (false, false),
        }
    }
}

/// The computed `background-position`, as an (x, y) pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackgroundPosition {
    pub x: BackgroundLength,
    pub y: BackgroundLength,
}

impl Default for BackgroundPosition {
    fn default() -> Self {
        // The CSS initial value is `0% 0%`.
        Self {
            x: BackgroundLength::Percent(0.0),
            y: BackgroundLength::Percent(0.0),
        }
    }
}

impl BackgroundPosition {
    /// Parse a one- or two-value `background-position`.
    ///
    /// Keywords are turned into percentages straight away: `left` is `0%`,
    /// `center` `50%`, `right` `100%` — which is exactly how CSS defines them,
    /// and lets paint time treat every case the same way.
    pub fn parse(val: &str, ctx: LengthContext) -> Option<Self> {
        let parts: Vec<&str> = split_components(val);
        if parts.is_empty() || parts.len() > 4 {
            return None;
        }

        let keyword = |s: &str| -> Option<(Option<bool>, BackgroundLength)> {
            // Returns (is_horizontal_axis, value); `None` axis means either.
            Some(match s.to_ascii_lowercase().as_str() {
                "left" => (Some(true), BackgroundLength::Percent(0.0)),
                "right" => (Some(true), BackgroundLength::Percent(1.0)),
                "top" => (Some(false), BackgroundLength::Percent(0.0)),
                "bottom" => (Some(false), BackgroundLength::Percent(1.0)),
                "center" => (None, BackgroundLength::Percent(0.5)),
                _ => return None,
            })
        };

        let mut x = None;
        let mut y = None;
        for part in &parts {
            if let Some((axis, value)) = keyword(part) {
                match axis {
                    Some(true) => x = Some(value),
                    Some(false) => y = Some(value),
                    None => {
                        if x.is_none() {
                            x = Some(value)
                        } else {
                            y = Some(value)
                        }
                    }
                }
            } else if let Some(value) = BackgroundLength::parse(part, ctx) {
                if x.is_none() {
                    x = Some(value);
                } else if y.is_none() {
                    y = Some(value);
                }
            } else {
                // An offset after an edge keyword (`right 20px`) — not supported.
                return None;
            }
        }

        Some(Self {
            x: x?,
            // A single value sets the horizontal position; the vertical
            // defaults to center, per CSS.
            y: y.unwrap_or(BackgroundLength::Percent(0.5)),
        })
    }
}

/// Parse a `background-size` value.
fn parse_background_size(val: &str, ctx: LengthContext) -> Option<BackgroundSize> {
    let val = val.trim();
    if val.eq_ignore_ascii_case("cover") {
        return Some(BackgroundSize::Cover);
    }
    if val.eq_ignore_ascii_case("contain") {
        return Some(BackgroundSize::Contain);
    }
    let parts = split_components(val);
    match parts.as_slice() {
        [w] => BackgroundLength::parse(w, ctx)
            .map(|w| BackgroundSize::Explicit(w, BackgroundLength::Auto)),
        [w, h] => {
            let w = BackgroundLength::parse(w, ctx)?;
            let h = BackgroundLength::parse(h, ctx)?;
            Some(BackgroundSize::Explicit(w, h))
        }
        _ => None,
    }
}

/// Split `s` at the first `sep` that is not inside parentheses.
fn split_outside_parens(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => return Some((&s[..i], &s[i + c.len_utf8()..])),
            _ => {}
        }
    }
    None
}

/// Extract the URL from a `url(...)` token, or `None` if it is not one.
///
/// Gradients (`linear-gradient(...)` and friends) are deliberately not handled;
/// they are a separate image type with no bitmap to fetch.
fn parse_url_token(s: &str) -> Option<String> {
    let s = s.trim();
    if !s.to_ascii_lowercase().starts_with("url(") || !s.ends_with(')') {
        return None;
    }
    let inner = &s[4..s.len() - 1];
    let url = inner.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

// ------ Cursor ------

/// The computed `cursor` property.
///
/// Only the keywords that map onto a platform cursor are kept; `url(...)`
/// cursors fall back to whatever keyword follows them in the list, which is
/// what the fallback syntax is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cursor {
    /// Let the element decide — links get a pointer, text gets an I-beam.
    #[default]
    Auto,
    Default,
    None,
    Pointer,
    Text,
    Crosshair,
    Move,
    Grab,
    Grabbing,
    NotAllowed,
    Progress,
    Wait,
    Help,
    ColResize,
    RowResize,
    NResize,
    EResize,
    SResize,
    WResize,
    NeResize,
    NwResize,
    SeResize,
    SwResize,
    EwResize,
    NsResize,
    ZoomIn,
    ZoomOut,
}

impl Cursor {
    /// Parse a single `cursor` keyword.
    fn parse_keyword(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "default" => Self::Default,
            "none" => Self::None,
            "pointer" => Self::Pointer,
            "text" | "vertical-text" => Self::Text,
            "crosshair" | "cell" => Self::Crosshair,
            "move" | "all-scroll" => Self::Move,
            "grab" => Self::Grab,
            "grabbing" => Self::Grabbing,
            "not-allowed" | "no-drop" => Self::NotAllowed,
            "progress" => Self::Progress,
            "wait" => Self::Wait,
            "help" | "context-menu" => Self::Help,
            "col-resize" => Self::ColResize,
            "row-resize" => Self::RowResize,
            "n-resize" => Self::NResize,
            "e-resize" => Self::EResize,
            "s-resize" => Self::SResize,
            "w-resize" => Self::WResize,
            "ne-resize" => Self::NeResize,
            "nw-resize" => Self::NwResize,
            "se-resize" => Self::SeResize,
            "sw-resize" => Self::SwResize,
            "ew-resize" => Self::EwResize,
            "ns-resize" => Self::NsResize,
            "zoom-in" => Self::ZoomIn,
            "zoom-out" => Self::ZoomOut,
            // `copy`, `alias` and friends have no distinct platform cursor
            // here; treat them as the default arrow rather than dropping the
            // declaration and inheriting something unrelated.
            "copy" | "alias" => Self::Default,
            _ => return None,
        })
    }

    /// Parse a `cursor` declaration, which may be a comma-separated fallback
    /// list ending in a keyword: `cursor: url(grab.cur), grab, pointer`.
    pub fn parse(val: &str) -> Option<Self> {
        split_top_level_commas(val)
            .into_iter()
            .find_map(|part| Self::parse_keyword(part.trim()))
    }
}

// ------ Borders ------

/// The computed `border-style` of one side.
///
/// The initial value is `None`, and CSS gives a side with no style a *used*
/// border width of zero — so `border-width: 2px` on its own paints nothing,
/// exactly as in a real browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    None,
    /// Same as `None` for painting; differs only in border-conflict resolution
    /// for collapsed tables.
    Hidden,
    Solid,
    Dashed,
    Dotted,
    /// Two solid lines with a gap between them.
    Double,
    /// The 3D styles. We have no lighting model, so these paint solid — which
    /// is what they degrade to at the 1–2px widths pages actually use.
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    /// Parse a `border-style` keyword; `None` for anything unrecognised.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "hidden" => Self::Hidden,
            "solid" => Self::Solid,
            "dashed" => Self::Dashed,
            "dotted" => Self::Dotted,
            "double" => Self::Double,
            "groove" => Self::Groove,
            "ridge" => Self::Ridge,
            "inset" => Self::Inset,
            "outset" => Self::Outset,
            _ => return None,
        })
    }

    /// Whether this style paints anything at all.
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::None | Self::Hidden)
    }
}

/// The `medium` border width, used when a side declares a style but no width.
const MEDIUM_BORDER_WIDTH: f32 = 3.0;

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
    /// `background-image` URL, unresolved. `None` means `none` — gradients are
    /// not bitmaps and are skipped rather than stored here.
    pub background_image: Option<String>,
    pub background_size: BackgroundSize,
    pub background_position: BackgroundPosition,
    pub background_repeat: BackgroundRepeat,
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
    /// `width`, `min-width` and `max-width` written as a percentage, kept as a
    /// fraction where `1.0` is 100%.
    ///
    /// A percentage is a share of the containing block, which the cascade
    /// cannot see — only layout knows how wide the parent turned out. Dropping
    /// them was invisible on a block box, which fills its parent anyway, and
    /// ruinous on a flex item: `width: 100%` fell back to the item's content
    /// size, which is how ja.wikipedia.org's 1580px header came out 300px wide
    /// with its text wrapping one character per line.
    pub width_percent: Option<f32>,
    pub min_width_percent: Option<f32>,
    pub max_width_percent: Option<f32>,
    /// The font size this element inherited, before its own `font-size` ran.
    ///
    /// `font-size: 150%` and `font-size: 1.5em` are both a multiple of the
    /// *parent's* size. Resolving them against `self.font_size` compounds when
    /// more than one rule sets the size on the same element.
    pub inherited_font_size: f32,
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
    /// Specified border width per side, before `border-style` is applied.
    /// Use [`ComputedValues::used_border_width`] for the value layout should see.
    pub border_width: [f32; 4],
    /// Per-side border colour; `None` means `currentColor`.
    pub border_color: [Option<[u8; 4]>; 4],
    /// Per-side `border-style`: [top, right, bottom, left].
    pub border_style: [BorderStyle; 4],
    pub border_radius: f32,
    // Typography
    pub text_style: TextStyleFlags,
    pub text_align: TextAlign,
    /// `direction` — the inline base direction; inherits.
    pub direction: Direction,
    pub visibility: Visibility,
    /// `cursor`; inherits, and `Auto` defers to the element's own role.
    pub cursor: Cursor,
    /// `z-index`; `None` is the initial `auto`.
    pub z_index: Option<i32>,
    /// `content` — what a `::before` or `::after` puts on the page. Empty on
    /// an ordinary element, which generates nothing.
    pub content: Content,
    /// `mask-image` — the picture whose alpha decides where this box's colour
    /// shows through. Still relative to the document base, like every other URL
    /// the cascade carries.
    ///
    /// An icon set is often drawn this way: one shape, recoloured per state by
    /// the box's own `background-color`, instead of one picture per colour.
    pub mask_image: Option<String>,
    pub mask_size: BackgroundSize,
    pub mask_position: BackgroundPosition,
    pub mask_repeat: BackgroundRepeat,
    /// Whether this box is a list item, and so is marked with a bullet or a
    /// number. Set by `display: list-item`; it is not a display type of its own
    /// here because a list item lays out as a block and only differs in having
    /// a marker.
    pub list_item: bool,
    /// `list-style-type`, or `None` where the page has not said.
    ///
    /// Held unresolved so that inheritance can tell "nobody said" from "this
    /// element asked for a disc": both would otherwise look like the initial
    /// value, and a disc inside a circled list would silently become a circle.
    pub list_style_type: Option<ListStyleType>,
    /// `list-style-position`, unresolved for the same reason.
    pub list_style_position: Option<ListStylePosition>,
    /// `transform` — how the box is moved, scaled or turned when it is painted.
    pub transform: Transform,
    /// `transform-origin` — the point that is done about.
    pub transform_origin: TransformOrigin,
    /// `filter` — the paint-time effects the box and its descendants go
    /// through. Does not inherit: a filtered box filters its subtree as one
    /// picture, rather than each descendant filtering itself again.
    pub filter: Filter,
    /// `animation-*` — the `@keyframes` rule this element is running, if any.
    pub animation: Animation,
    /// `transition-*` — which of this element's properties ease into their new
    /// values rather than jumping to them.
    pub transitions: Transitions,
    // CSS Custom Properties (CSS variables --*)
    pub custom_properties: rustc_hash::FxHashMap<String, String>,
}

/// A `transform` function, before it is resolved against a box.
///
/// Kept as written rather than folded into a matrix straight away: a
/// percentage translate is a fraction of the box's own size, which is not
/// known until the box has been laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformFn {
    Translate {
        x: LengthOrPercent,
        y: LengthOrPercent,
    },
    Scale {
        x: f32,
        y: f32,
    },
    Rotate {
        radians: f32,
    },
    /// `matrix(a, b, c, d, e, f)`, and anything else that reduces to one.
    Matrix([f32; 6]),
}

/// A length that may be written as a percentage of something.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LengthOrPercent {
    pub px: f32,
    /// A fraction, so `50%` is 0.5.
    pub percent: f32,
}

impl LengthOrPercent {
    /// Resolve against the length a percentage refers to.
    pub fn resolve(&self, basis: f32) -> f32 {
        self.px + self.percent * basis
    }

    fn parse(token: &str, ctx: LengthContext) -> Option<Self> {
        let token = token.trim();
        if let Some(percent) = token.strip_suffix('%') {
            return percent.trim().parse::<f32>().ok().map(|p| Self {
                px: 0.0,
                percent: p / 100.0,
            });
        }
        parse_length_ctx(token, ctx).map(|px| Self { px, percent: 0.0 })
    }
}

/// An affine transform, as the painter needs it.
///
/// The same six numbers CSS and SVG use: `x' = a*x + c*y + e`,
/// `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self` followed by `other`.
    pub fn then(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Where this transform sends a point.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// Whether this only moves and scales, leaving the axes where they are.
    ///
    /// The compositor paints axis-aligned rectangles and upright glyphs, so a
    /// rotation or a skew is not something it can carry out.
    pub fn is_axis_aligned(&self) -> bool {
        self.b.abs() < 1e-4 && self.c.abs() < 1e-4
    }
}

/// The computed `transform` property.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transform {
    functions: Vec<TransformFn>,
}

/// The point a transform is applied about — the computed `transform-origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformOrigin {
    pub x: LengthOrPercent,
    pub y: LengthOrPercent,
}

impl Default for TransformOrigin {
    /// The centre of the box, as CSS says.
    fn default() -> Self {
        Self {
            x: LengthOrPercent {
                px: 0.0,
                percent: 0.5,
            },
            y: LengthOrPercent {
                px: 0.0,
                percent: 0.5,
            },
        }
    }
}

impl TransformOrigin {
    fn parse(value: &str, ctx: LengthContext) -> Self {
        let mut origin = Self::default();
        let fraction = |percent: f32| LengthOrPercent { px: 0.0, percent };
        let mut positional = Vec::new();

        for token in value.split_whitespace() {
            match token.to_ascii_lowercase().as_str() {
                "left" => origin.x = fraction(0.0),
                "right" => origin.x = fraction(1.0),
                "top" => origin.y = fraction(0.0),
                "bottom" => origin.y = fraction(1.0),
                // `center` alone means both axes; beside another keyword it
                // only fills the axis that keyword did not.
                "center" => {}
                other => {
                    if let Some(length) = LengthOrPercent::parse(other, ctx) {
                        positional.push(length);
                    }
                }
            }
        }
        if let Some(x) = positional.first() {
            origin.x = *x;
        }
        if let Some(y) = positional.get(1) {
            origin.y = *y;
        }
        origin
    }
}

impl Transform {
    pub fn is_none(&self) -> bool {
        self.functions.is_empty()
    }

    /// Parse a `transform` value.
    pub fn parse(value: &str, ctx: LengthContext) -> Self {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("none") {
            return Self::default();
        }

        let mut functions = Vec::new();
        let mut rest = value;
        while let Some(open) = rest.find('(') {
            let name = rest[..open].trim().to_ascii_lowercase();
            let Some(close) = rest[open..].find(')') else {
                break;
            };
            let args: Vec<&str> = rest[open + 1..open + close]
                .split(',')
                .map(str::trim)
                .collect();
            rest = &rest[open + close + 1..];

            let number = |index: usize| -> Option<f32> {
                args.get(index).and_then(|a| a.parse::<f32>().ok())
            };
            let length = |index: usize| -> Option<LengthOrPercent> {
                args.get(index).and_then(|a| LengthOrPercent::parse(a, ctx))
            };

            let parsed = match name.as_str() {
                "translate" => length(0).map(|x| TransformFn::Translate {
                    x,
                    y: length(1).unwrap_or_default(),
                }),
                "translatex" => length(0).map(|x| TransformFn::Translate {
                    x,
                    y: LengthOrPercent::default(),
                }),
                "translatey" => length(0).map(|y| TransformFn::Translate {
                    x: LengthOrPercent::default(),
                    y,
                }),
                "scale" => number(0).map(|x| TransformFn::Scale {
                    x,
                    // `scale(2)` scales both axes.
                    y: number(1).unwrap_or(x),
                }),
                "scalex" => number(0).map(|x| TransformFn::Scale { x, y: 1.0 }),
                "scaley" => number(0).map(|y| TransformFn::Scale { x: 1.0, y }),
                "rotate" => parse_angle(args.first().copied().unwrap_or(""))
                    .map(|radians| TransformFn::Rotate { radians }),
                "matrix" => {
                    let values: Vec<f32> = (0..6).filter_map(number).collect();
                    (values.len() == 6).then(|| {
                        TransformFn::Matrix([
                            values[0], values[1], values[2], values[3], values[4], values[5],
                        ])
                    })
                }
                // 3D transforms, skew and perspective are not modelled; an
                // unrecognised function leaves the rest of the list intact.
                _ => None,
            };
            functions.extend(parsed);
        }

        Self { functions }
    }

    /// The matrix this comes to for a box of the given size.
    ///
    /// Percentages resolve against the box, and the whole thing happens about
    /// `origin` — which is why the box has to be known first.
    pub fn resolve(&self, width: f32, height: f32, origin: TransformOrigin) -> Matrix {
        if self.functions.is_empty() {
            return Matrix::IDENTITY;
        }

        let mut matrix = Matrix::IDENTITY;
        // CSS applies the right-most function to the point first, as though
        // each function nested inside the one to its left. Composing "this
        // step, then everything gathered so far" while reading left to right
        // builds exactly that.
        for function in self.functions.iter() {
            let step = match *function {
                TransformFn::Translate { x, y } => Matrix {
                    e: x.resolve(width),
                    f: y.resolve(height),
                    ..Matrix::IDENTITY
                },
                TransformFn::Scale { x, y } => Matrix {
                    a: x,
                    d: y,
                    ..Matrix::IDENTITY
                },
                TransformFn::Rotate { radians } => Matrix {
                    a: radians.cos(),
                    b: radians.sin(),
                    c: -radians.sin(),
                    d: radians.cos(),
                    ..Matrix::IDENTITY
                },
                TransformFn::Matrix([a, b, c, d, e, f]) => Matrix { a, b, c, d, e, f },
            };
            matrix = step.then(&matrix);
        }

        // Move the point everything turns about to the origin, transform, and
        // move it back.
        let (ox, oy) = (origin.x.resolve(width), origin.y.resolve(height));
        let to_origin = Matrix {
            e: -ox,
            f: -oy,
            ..Matrix::IDENTITY
        };
        let back = Matrix {
            e: ox,
            f: oy,
            ..Matrix::IDENTITY
        };
        to_origin.then(&matrix).then(&back)
    }
}

/// Parse a CSS angle into radians.
fn parse_angle(token: &str) -> Option<f32> {
    let token = token.trim().to_ascii_lowercase();
    let (number, factor) = if let Some(n) = token.strip_suffix("deg") {
        (n, std::f32::consts::PI / 180.0)
    } else if let Some(n) = token.strip_suffix("rad") {
        (n, 1.0)
    } else if let Some(n) = token.strip_suffix("turn") {
        (n, std::f32::consts::TAU)
    } else if let Some(n) = token.strip_suffix("grad") {
        (n, std::f32::consts::PI / 200.0)
    } else {
        (token.as_str(), std::f32::consts::PI / 180.0)
    };
    number.trim().parse::<f32>().ok().map(|n| n * factor)
}

/// The `content` property: what a generated box puts on the page.
///
/// Only the parts this engine can produce are modelled — literal strings and
/// `attr()`. Counters, `url()` images and quotes are recognised as content we
/// cannot build and leave the box empty, which is better than printing the
/// source text of the function.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Content {
    parts: Vec<ContentPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ContentPart {
    Text(String),
    /// `attr(href)` — the value of that attribute on the originating element.
    Attr(String),
}

/// Which generated box a style belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PseudoKind {
    Before,
    After,
}

impl PseudoKind {
    /// The name as it is written in a selector.
    pub fn name(self) -> &'static str {
        match self {
            PseudoKind::Before => "before",
            PseudoKind::After => "after",
        }
    }

    /// The two kinds, in the order they are attached to their element.
    pub fn all() -> [PseudoKind; 2] {
        [PseudoKind::Before, PseudoKind::After]
    }
}

/// Where pseudo-element styles live in the style map.
///
/// A pseudo-element has no DOM node, so it has no id of its own. Its style is
/// keyed off the element it belongs to, above any id the arena can hand out —
/// which keeps one map, and so keeps every function that carries styles around
/// unchanged.
pub const PSEUDO_STYLE_BASE: u32 = 1 << 28;

/// The style-map key for one element's generated box.
pub fn pseudo_style_key(node_id: u32, kind: PseudoKind) -> u32 {
    let slot = match kind {
        PseudoKind::Before => 0,
        PseudoKind::After => 1,
    };
    PSEUDO_STYLE_BASE + node_id * 2 + slot
}

impl Content {
    /// Parse a `content` value.
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty()
            || value.eq_ignore_ascii_case("none")
            || value.eq_ignore_ascii_case("normal")
        {
            return Self::default();
        }

        let mut parts = Vec::new();
        let chars: Vec<char> = value.chars().collect();
        let mut i = 0usize;

        while i < chars.len() {
            match chars[i] {
                c if c.is_whitespace() => i += 1,
                quote @ ('"' | '\'') => {
                    i += 1;
                    let mut text = String::new();
                    while i < chars.len() && chars[i] != quote {
                        if chars[i] == '\\' {
                            i += 1;
                            text.push_str(&read_escape(&chars, &mut i));
                            continue;
                        }
                        text.push(chars[i]);
                        i += 1;
                    }
                    i += 1; // closing quote
                    parts.push(ContentPart::Text(text));
                }
                _ => {
                    // A function or keyword: read to the end of the token, and
                    // keep only `attr()`.
                    let start = i;
                    let mut depth = 0usize;
                    while i < chars.len() {
                        match chars[i] {
                            '(' => depth += 1,
                            ')' => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            c if c.is_whitespace() && depth == 0 => break,
                            _ => {}
                        }
                        i += 1;
                    }
                    let token: String = chars[start..i].iter().collect();
                    let lower = token.to_ascii_lowercase();
                    if let Some(rest) = lower.strip_prefix("attr(") {
                        if let Some(name) = rest.strip_suffix(')') {
                            let name = name.trim().trim_matches(['"', '\'']);
                            if !name.is_empty() {
                                parts.push(ContentPart::Attr(name.to_string()));
                            }
                        }
                    }
                    // Anything else — counters, url(), open-quote — is content
                    // this engine cannot build, and is left out.
                }
            }
        }

        Self { parts }
    }

    /// Whether this generates a box at all.
    ///
    /// `content: ""` does generate one — an empty box that CSS often sizes and
    /// colours to draw a shape — so an empty list and an empty string are not
    /// the same thing.
    pub fn generates_box(&self) -> bool {
        !self.parts.is_empty()
    }

    /// The text this puts on the page, resolving `attr()` through `attribute`.
    pub fn resolve(&self, attribute: &impl Fn(&str) -> Option<String>) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                ContentPart::Text(text) => text.clone(),
                ContentPart::Attr(name) => attribute(name).unwrap_or_default(),
            })
            .collect()
    }
}

/// The picture a `mask` or `mask-image` value names, if it names one.
///
/// `none` and the shapes we cannot draw — gradients, `linear-gradient()` used
/// as a fade — leave nothing to mask with, and a box with no mask paints its
/// background the ordinary way.
fn mask_url(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case("none") {
        return None;
    }
    split_top_level_commas(value)
        .into_iter()
        .find_map(parse_url_token)
}

/// Read one CSS escape sequence, having consumed the backslash.
///
/// `\f101` is how icon fonts are addressed, so a hex escape has to become the
/// character it names rather than the letters that spell it.
fn read_escape(chars: &[char], i: &mut usize) -> String {
    let mut hex = String::new();
    while *i < chars.len() && hex.len() < 6 && chars[*i].is_ascii_hexdigit() {
        hex.push(chars[*i]);
        *i += 1;
    }
    if hex.is_empty() {
        // An escaped literal, such as `\"`.
        if *i < chars.len() {
            let escaped = chars[*i];
            *i += 1;
            return escaped.to_string();
        }
        return String::new();
    }
    // One whitespace character after the digits ends the escape and is consumed.
    if *i < chars.len() && chars[*i].is_whitespace() {
        *i += 1;
    }
    u32::from_str_radix(&hex, 16)
        .ok()
        .and_then(char::from_u32)
        .map(|c| c.to_string())
        .unwrap_or_default()
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
    /// The initial value: the inline start edge, which `direction` decides.
    #[default]
    Start,
    /// The inline end edge.
    End,
    Left,
    Center,
    Right,
    Justify,
}

impl TextAlign {
    /// Resolve the writing-mode-relative values against a direction.
    ///
    /// `start` and `end` are the ones that depend on it: in right-to-left text
    /// the start edge is the right one.
    pub fn resolve(self, direction: Direction) -> Self {
        match (self, direction) {
            (Self::Start, Direction::Ltr) => Self::Left,
            (Self::Start, Direction::Rtl) => Self::Right,
            (Self::End, Direction::Ltr) => Self::Right,
            (Self::End, Direction::Rtl) => Self::Left,
            (other, _) => other,
        }
    }
}

/// The computed `direction` property — the inline base direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

impl Direction {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ltr" => Some(Self::Ltr),
            "rtl" => Some(Self::Rtl),
            _ => None,
        }
    }

    /// The base embedding level: even for left-to-right, odd for right-to-left.
    pub fn base_level(self) -> u8 {
        match self {
            Self::Ltr => 0,
            Self::Rtl => 1,
        }
    }
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
            background_image: None,
            background_size: BackgroundSize::Auto,
            background_position: BackgroundPosition::default(),
            background_repeat: BackgroundRepeat::Repeat,
            color: None,
            font_size: 16.0,
            inherited_font_size: 16.0,
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
            width_percent: None,
            min_width_percent: None,
            max_width_percent: None,
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
            border_color: [None; 4],
            border_style: [BorderStyle::None; 4],
            border_radius: 0.0,
            text_style: TextStyleFlags::default(),
            text_align: TextAlign::Start,
            direction: Direction::Ltr,
            visibility: Visibility::Visible,
            cursor: Cursor::Auto,
            z_index: None,
            content: Content::default(),
            mask_image: None,
            mask_size: BackgroundSize::Auto,
            mask_position: BackgroundPosition::default(),
            mask_repeat: BackgroundRepeat::Repeat,
            list_item: false,
            list_style_type: None,
            list_style_position: None,
            transform: Transform::default(),
            transform_origin: TransformOrigin::default(),
            filter: Filter::default(),
            animation: Animation::default(),
            transitions: Transitions::default(),
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
    /// The border widths layout should use.
    ///
    /// A side whose `border-style` is `none` or `hidden` contributes nothing to
    /// the box, however wide `border-width` says it is — so `border-width: 2px`
    /// without a style occupies no space and paints nothing, matching browsers.
    pub fn used_border_width(&self) -> [f32; 4] {
        let mut used = [0.0; 4];
        for i in 0..4 {
            if self.border_style[i].is_visible() {
                used[i] = self.border_width[i];
            }
        }
        used
    }

    /// The border colour of one side, resolving the `currentColor` initial
    /// value against the element's own `color`.
    pub fn resolved_border_color(&self, side: usize) -> [u8; 4] {
        self.border_color[side].unwrap_or(self.color.unwrap_or([0, 0, 0, 255]))
    }

    /// Whether any side paints a border.
    pub fn has_visible_border(&self) -> bool {
        (0..4).any(|i| self.border_style[i].is_visible() && self.border_width[i] > 0.0)
    }

    /// Apply a `background` shorthand.
    ///
    /// The shorthand resets every background property it can carry, so an
    /// omitted component goes back to its initial value — this is what makes
    /// `background: #fff` clear an image inherited from a less specific rule.
    ///
    /// Only the first layer of a comma-separated list is painted; later layers
    /// sit behind it and are far less common than the single-layer form.
    fn apply_background_shorthand(&mut self, val: &str, ctx: LengthContext) {
        self.background_image = None;
        self.background_size = BackgroundSize::Auto;
        self.background_position = BackgroundPosition::default();
        self.background_repeat = BackgroundRepeat::Repeat;

        let layer = split_top_level_commas(val)
            .into_iter()
            .next()
            .unwrap_or(val)
            .trim();

        // `background-position / background-size` is the one ordered pair in
        // the shorthand, so split it off before scanning the rest.
        let (before_slash, after_slash) = match split_outside_parens(layer, '/') {
            Some((a, b)) => (a, Some(b)),
            None => (layer, None),
        };

        if let Some(size_part) = after_slash {
            if let Some(size) = parse_background_size(size_part, ctx) {
                self.background_size = size;
            }
        }

        let mut position_tokens: Vec<&str> = Vec::new();
        for token in split_components(before_slash) {
            if let Some(url) = parse_url_token(token) {
                self.background_image = Some(url);
            } else if let Some(repeat) = BackgroundRepeat::parse(token) {
                self.background_repeat = repeat;
            } else if matches!(
                token.to_ascii_lowercase().as_str(),
                "scroll" | "fixed" | "local" | "border-box" | "padding-box" | "content-box"
            ) {
                // attachment / origin / clip — parsed so they are not mistaken
                // for a colour, but not yet acted on.
            } else if let Some(color) = parse_color_value(token) {
                let (r, g, b, a) = color.to_rgba();
                self.background_color = Some([r, g, b, a]);
            } else {
                position_tokens.push(token);
            }
        }

        if !position_tokens.is_empty() {
            if let Some(pos) = BackgroundPosition::parse(&position_tokens.join(" "), ctx) {
                self.background_position = pos;
            }
        }
    }

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
    /// This element's value for an animatable property, written the way CSS
    /// would write it.
    ///
    /// A transition has to notice that a value changed and then blend between
    /// the old one and the new. Both need a written form: the interpolation
    /// works on values as text, so that one routine can serve every property
    /// rather than each one growing its own blend.
    ///
    /// `None` means the property is not one this engine animates, or the
    /// element does not set it.
    pub fn animatable_value(&self, property: &str) -> Option<String> {
        let color = |rgba: Option<[u8; 4]>| {
            rgba.map(|[r, g, b, a]| format!("rgba({r}, {g}, {b}, {})", a as f32 / 255.0))
        };
        let px = |value: f32| Some(format!("{value}px"));

        match property {
            "color" => color(self.color),
            "background-color" => color(self.background_color),
            "border-color" => color(self.border_color[0]),
            "border-radius" => px(self.border_radius),
            "border-width" => px(self.border_width[0]),
            "width" => self.width.and_then(px),
            "height" => self.height.and_then(px),
            "min-width" => self.min_width.and_then(px),
            "max-width" => self.max_width.and_then(px),
            "top" => self.offset_top.and_then(px),
            "right" => self.offset_right.and_then(px),
            "bottom" => self.offset_bottom.and_then(px),
            "left" => self.offset_left.and_then(px),
            "margin-top" => px(self.margin[0]),
            "margin-right" => px(self.margin[1]),
            "margin-bottom" => px(self.margin[2]),
            "margin-left" => px(self.margin[3]),
            "padding-top" => px(self.padding[0]),
            "padding-right" => px(self.padding[1]),
            "padding-bottom" => px(self.padding[2]),
            "padding-left" => px(self.padding[3]),
            "font-size" => px(self.font_size),
            "line-height" => Some(format!("{}", self.line_height)),
            "row-gap" => px(self.row_gap),
            "column-gap" => px(self.column_gap),
            "flex-grow" => Some(format!("{}", self.flex_grow)),
            "flex-shrink" => Some(format!("{}", self.flex_shrink)),
            _ => None,
        }
    }

    /// Apply one `transition-*` longhand.
    ///
    /// The longhands are four parallel lists that line up by position, so a
    /// value is written into every entry the property list has — and the
    /// property list itself is what decides how many entries there are.
    fn apply_transition_longhand(&mut self, property: &str, value: &str) {
        use animation::TransitionEntry;

        let parts = animation::split_top_level_commas(value);
        if property == "transition-property" {
            self.transitions.entries = parts
                .iter()
                .map(|name| TransitionEntry {
                    property: name.to_ascii_lowercase(),
                    duration: 0.0,
                    delay: 0.0,
                    easing: Easing::default(),
                })
                .collect();
            return;
        }
        // A duration written before the property list still has to land
        // somewhere, so an entry is made for it.
        if self.transitions.entries.is_empty() {
            self.transitions.entries.push(TransitionEntry {
                property: "all".to_string(),
                duration: 0.0,
                delay: 0.0,
                easing: Easing::default(),
            });
        }

        for (index, entry) in self.transitions.entries.iter_mut().enumerate() {
            // A shorter list repeats, as CSS says.
            let Some(part) = parts.get(index % parts.len().max(1)) else {
                break;
            };
            match property {
                "transition-duration" => {
                    entry.duration = animation::parse_time(part).unwrap_or(0.0)
                }
                "transition-delay" => entry.delay = animation::parse_time(part).unwrap_or(0.0),
                "transition-timing-function" => {
                    if let Some(easing) = Easing::parse(part) {
                        entry.easing = easing;
                    }
                }
                _ => {}
            }
        }
    }

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
                    // A list item is a block that carries a marker. Keeping it
                    // out of `DisplayType` means every layout path that already
                    // handles a block handles this too, rather than each one
                    // growing an arm that says "and list items as well".
                    "list-item" => {
                        self.list_item = true;
                        DisplayType::Block
                    }
                    _ => self.display,
                };
                if val != "list-item" {
                    self.list_item = false;
                }
            }
            "width" => {
                self.width_percent = parse_percentage(val);
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
            "background-color" => {
                if let Some(color) = parse_color_value(val) {
                    let (r, g, b, a) = color.to_rgba();
                    self.background_color = Some([r, g, b, a]);
                }
            }
            "background" => self.apply_background_shorthand(val, ctx),
            "background-image" => {
                // `none` and gradients both leave us with nothing to paint.
                self.background_image = if val.eq_ignore_ascii_case("none") {
                    None
                } else {
                    split_top_level_commas(val)
                        .into_iter()
                        .find_map(parse_url_token)
                };
            }
            "background-size" => {
                if let Some(size) = parse_background_size(val, ctx) {
                    self.background_size = size;
                }
            }
            "background-position" => {
                if let Some(pos) = BackgroundPosition::parse(val, ctx) {
                    self.background_position = pos;
                }
            }
            "background-repeat" => {
                // The two-value form (`repeat no-repeat`) names each axis.
                let parts = split_components(val);
                self.background_repeat = match parts.as_slice() {
                    [one] => BackgroundRepeat::parse(one).unwrap_or(self.background_repeat),
                    [h, v] => {
                        let h_repeats = !h.eq_ignore_ascii_case("no-repeat");
                        let v_repeats = !v.eq_ignore_ascii_case("no-repeat");
                        match (h_repeats, v_repeats) {
                            (true, true) => BackgroundRepeat::Repeat,
                            (true, false) => BackgroundRepeat::RepeatX,
                            (false, true) => BackgroundRepeat::RepeatY,
                            (false, false) => BackgroundRepeat::NoRepeat,
                        }
                    }
                    _ => self.background_repeat,
                };
            }
            "color" => {
                if let Some(color) = parse_color_value(val) {
                    let (r, g, b, a) = color.to_rgba();
                    self.color = Some([r, g, b, a]);
                }
            }
            "font-size" => {
                // Both `%` and `em` here are multiples of the parent's size,
                // not of whatever a previous rule already set on this element.
                let base = LengthContext {
                    font_size: self.inherited_font_size,
                    ..ctx
                };
                if let Some(fraction) = parse_percentage(val) {
                    self.font_size = self.inherited_font_size * fraction;
                } else if let Some(v) = parse_length_ctx(val, base) {
                    self.font_size = v;
                }
            }
            "font-family" => {
                let stack = normalize_font_family(val);
                if !stack.is_empty() {
                    self.font_family = stack;
                }
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
                    "sticky" => PositionType::Sticky,
                    // `fixed` is not implemented; treating it as static leaves
                    // the box in flow rather than dropping it at the origin.
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
                self.min_width_percent = parse_percentage(val);
                self.min_width = parse_length(val);
            }
            "max-width" => {
                self.max_width_percent = parse_percentage(val);
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
            "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
                let sides: &[usize] = match prop.as_str() {
                    "border-top" => &[0],
                    "border-right" => &[1],
                    "border-bottom" => &[2],
                    "border-left" => &[3],
                    _ => &[0, 1, 2, 3],
                };
                let parsed = parse_border_shorthand(val, ctx);
                for &i in sides {
                    // A shorthand resets the sides it names, so an omitted
                    // component goes back to its initial value rather than
                    // keeping whatever an earlier declaration left there.
                    self.border_style[i] = parsed.style.unwrap_or_default();
                    self.border_color[i] = parsed.color;
                    self.border_width[i] =
                        parsed
                            .width
                            .unwrap_or(if self.border_style[i].is_visible() {
                                MEDIUM_BORDER_WIDTH
                            } else {
                                0.0
                            });
                }
            }
            "border-top-width"
            | "border-right-width"
            | "border-bottom-width"
            | "border-left-width" => {
                if let Some(w) = parse_length_ctx(val, ctx) {
                    self.border_width[border_side_index(&prop)] = w;
                }
            }
            "border-top-color"
            | "border-right-color"
            | "border-bottom-color"
            | "border-left-color" => {
                if let Some(c) = parse_color_value(val) {
                    let (r, g, b, a) = c.to_rgba();
                    self.border_color[border_side_index(&prop)] = Some([r, g, b, a]);
                }
            }
            "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style" => {
                if let Some(s) = BorderStyle::parse(val) {
                    self.border_style[border_side_index(&prop)] = s;
                }
            }
            "border-width" => {
                self.border_width = parse_box_four(val, self.border_width);
            }
            "border-color" => {
                self.border_color = parse_box_four_generic(val, self.border_color, |p| {
                    parse_color_value(p).map(|c| {
                        let (r, g, b, a) = c.to_rgba();
                        Some([r, g, b, a])
                    })
                });
            }
            "border-style" => {
                self.border_style =
                    parse_box_four_generic(val, self.border_style, BorderStyle::parse);
            }
            "cursor" => {
                if let Some(c) = Cursor::parse(val) {
                    self.cursor = c;
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
            "content" => {
                self.content = Content::parse(val);
            }
            // The vendor-prefixed spellings are the ones a page written for
            // Safari or an older Chrome sends, and the two are written side by
            // side rather than either standing alone — so they are one property
            // as far as we are concerned.
            "mask-image" | "-webkit-mask-image" => {
                self.mask_image = mask_url(val);
            }
            "mask-size" | "-webkit-mask-size" => {
                if let Some(size) = parse_background_size(val, ctx) {
                    self.mask_size = size;
                }
            }
            "mask-position" | "-webkit-mask-position" => {
                if let Some(pos) = BackgroundPosition::parse(val, ctx) {
                    self.mask_position = pos;
                }
            }
            "mask-repeat" | "-webkit-mask-repeat" => {
                if let Some(repeat) = BackgroundRepeat::parse(val) {
                    self.mask_repeat = repeat;
                }
            }
            "mask" | "-webkit-mask" => {
                // The shorthand resets what it does not mention, which is what
                // makes `mask: none` clear an icon set further up the cascade.
                self.mask_image = mask_url(val);
                self.mask_size = BackgroundSize::Auto;
                self.mask_position = BackgroundPosition::default();
                self.mask_repeat = BackgroundRepeat::Repeat;
                for part in split_components(val) {
                    if let Some(repeat) = BackgroundRepeat::parse(part) {
                        self.mask_repeat = repeat;
                    }
                }
            }
            "list-style-type" => {
                self.list_style_type = ListStyleType::parse(val);
            }
            "list-style-position" => {
                self.list_style_position = ListStylePosition::parse(val);
            }
            "list-style" => {
                // The shorthand takes a type, a position and an image in any
                // order. We have no marker images, so a `url(...)` is read as
                // the type it is standing in for having none.
                for part in val.split_whitespace() {
                    if let Some(position) = ListStylePosition::parse(part) {
                        self.list_style_position = Some(position);
                    } else if let Some(kind) = ListStyleType::parse(part) {
                        self.list_style_type = Some(kind);
                    }
                }
            }
            "transform" => {
                self.transform = Transform::parse(val, ctx);
            }
            "transform-origin" => {
                self.transform_origin = TransformOrigin::parse(val, ctx);
            }
            "filter" => {
                self.filter = Filter::parse(val, ctx);
            }
            "transition" => {
                self.transitions = Transitions::parse_shorthand(val);
            }
            "transition-property"
            | "transition-duration"
            | "transition-delay"
            | "transition-timing-function" => {
                self.apply_transition_longhand(&prop, val);
            }
            "animation" | "-webkit-animation" => {
                self.animation = Animation::parse_shorthand(val);
            }
            "animation-name" => {
                self.animation.name = val
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
            "animation-duration" => {
                self.animation.duration = animation::parse_time(val).unwrap_or(0.0);
            }
            "animation-delay" => {
                self.animation.delay = animation::parse_time(val).unwrap_or(0.0);
            }
            "animation-timing-function" => {
                if let Some(easing) = Easing::parse(val) {
                    self.animation.easing = easing;
                }
            }
            "animation-iteration-count" => {
                self.animation.iterations = if val.trim().eq_ignore_ascii_case("infinite") {
                    None
                } else {
                    Some(val.trim().parse::<f32>().unwrap_or(1.0).max(0.0))
                };
            }
            "animation-direction" => {
                if let Some(direction) = animation::parse_direction(val) {
                    self.animation.direction = direction;
                }
            }
            "animation-fill-mode" => {
                if let Some(fill) = animation::parse_fill_mode(val) {
                    self.animation.fill_mode = fill;
                }
            }
            "z-index" => {
                // `auto` (and anything unparseable) leaves the element in
                // document order, which `None` represents.
                self.z_index = val.trim().parse::<i32>().ok();
            }
            "visibility" => {
                self.visibility = match val.to_ascii_lowercase().as_str() {
                    "hidden" => Visibility::Hidden,
                    "collapse" => Visibility::Collapse,
                    _ => Visibility::Visible,
                };
            }
            "direction" => {
                if let Some(d) = Direction::parse(val) {
                    self.direction = d;
                }
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
                    "start" => TextAlign::Start,
                    "end" => TextAlign::End,
                    "left" => TextAlign::Left,
                    _ => self.text_align,
                };
            }
            _ => {}
        }

        self
    }
}

/// Helper to parse CSS border shorthand like "1px solid #ccc"
/// The three components a `border` shorthand can carry, each absent when the
/// author omitted it.
struct BorderShorthand {
    width: Option<f32>,
    style: Option<BorderStyle>,
    color: Option<[u8; 4]>,
}

/// Parse a `border` / `border-<side>` shorthand.
///
/// The components may appear in any order, so each token is offered to the
/// style keywords first — `solid` and friends would otherwise be swallowed by
/// the colour parser as unknown names.
fn parse_border_shorthand(val: &str, ctx: LengthContext) -> BorderShorthand {
    let mut out = BorderShorthand {
        width: None,
        style: None,
        color: None,
    };
    for part in split_components(val) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if out.style.is_none() {
            if let Some(s) = BorderStyle::parse(part) {
                out.style = Some(s);
                continue;
            }
        }
        if out.width.is_none() {
            // The `thin`/`medium`/`thick` keywords are widths, not lengths.
            let keyword = match part.to_ascii_lowercase().as_str() {
                "thin" => Some(1.0),
                "medium" => Some(MEDIUM_BORDER_WIDTH),
                "thick" => Some(5.0),
                _ => None,
            };
            if let Some(w) = keyword.or_else(|| parse_length_ctx(part, ctx)) {
                out.width = Some(w);
                continue;
            }
        }
        if out.color.is_none() {
            if let Some(c) = parse_color_value(part) {
                let (r, g, b, a) = c.to_rgba();
                out.color = Some([r, g, b, a]);
            }
        }
    }
    out
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

/// A percentage value as a fraction, where `1.0` is 100%.
///
/// `None` for anything that is not a bare percentage — a length, a keyword, or
/// arithmetic. Percentages resolve against something the cascade often cannot
/// see, so they are read out separately rather than folded into a length.
pub(crate) fn parse_percentage(s: &str) -> Option<f32> {
    let pct = s.trim().strip_suffix('%')?;
    let value: f32 = pct.trim().parse().ok()?;
    value.is_finite().then_some(value / 100.0)
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
    /// Page zoom. Every absolute length is multiplied by it, which is what
    /// makes a zoomed page reflow — the window stays the size it is, and the
    /// page's own lengths grow inside it.
    pub zoom: f32,
}

// ------ `@supports` ------

/// Every property name the cascade knows how to apply.
///
/// This is what `@supports (prop: value)` answers from. Claiming a property we
/// then ignore is worse than admitting we lack it: a page that asks about
/// something it has a fallback for is offering us the fallback, and taking the
/// branch we cannot paint gets us the blank version of the page. So a name
/// belongs on this list once the property is painted, and not before.
const SUPPORTED_PROPERTIES: &[&str] = &[
    "-webkit-animation",
    "align-content",
    "align-items",
    "align-self",
    "animation",
    "animation-delay",
    "animation-direction",
    "animation-duration",
    "animation-fill-mode",
    "animation-iteration-count",
    "animation-name",
    "animation-timing-function",
    "background",
    "background-color",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "border",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-style",
    "border-bottom-width",
    "border-color",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-radius",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-style",
    "border-top-width",
    "border-width",
    "bottom",
    "box-sizing",
    "clear",
    "color",
    "column-gap",
    "content",
    "cursor",
    "direction",
    "display",
    "filter",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "float",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "gap",
    "grid-column-gap",
    "grid-row-gap",
    "grid-template-columns",
    "grid-template-rows",
    "height",
    "justify-content",
    "justify-items",
    "left",
    "letter-spacing",
    "line-height",
    "list-style",
    "list-style-position",
    "list-style-type",
    "-webkit-mask",
    "-webkit-mask-image",
    "-webkit-mask-position",
    "-webkit-mask-repeat",
    "-webkit-mask-size",
    "mask",
    "mask-image",
    "mask-position",
    "mask-repeat",
    "mask-size",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-width",
    "min-width",
    "order",
    "overflow",
    "overflow-x",
    "overflow-y",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "position",
    "right",
    "row-gap",
    "text-align",
    "text-decoration",
    "text-decoration-line",
    "text-transform",
    "top",
    "transform",
    "transform-origin",
    "transition",
    "transition-delay",
    "transition-duration",
    "transition-property",
    "transition-timing-function",
    "visibility",
    "width",
    "word-spacing",
    "z-index",
];

/// Value functions we can evaluate. Anything else makes a value unsupported.
const SUPPORTED_VALUE_FUNCTIONS: &[&str] = &[
    "calc",
    "min",
    "max",
    "clamp",
    "var",
    "url",
    "attr",
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "linear-gradient",
    "radial-gradient",
    "translate",
    "translatex",
    "translatey",
    "translate3d",
    "scale",
    "scalex",
    "scaley",
    "rotate",
    "skew",
    "skewx",
    "skewy",
    "matrix",
    "blur",
    "brightness",
    "contrast",
    "grayscale",
    "invert",
    "opacity",
    "saturate",
    "sepia",
    "hue-rotate",
    "drop-shadow",
    "repeat",
    "minmax",
    "fit-content",
    "cubic-bezier",
    "steps",
    "counter",
    "format",
    "local",
];

/// Whether `@supports (property: value)` should hold.
///
/// True when the cascade has a rule for the property *and* the value uses only
/// functions we can evaluate — `width: round(1.5px, 1px)` is a property we know
/// carrying arithmetic we do not.
pub fn supports_declaration(property: &str, value: &str) -> bool {
    let property = property.trim().to_ascii_lowercase();
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    // A custom property accepts any value by definition.
    if property.starts_with("--") {
        return true;
    }
    if !SUPPORTED_PROPERTIES.contains(&property.as_str()) {
        return false;
    }
    !uses_an_unknown_function(value)
}

/// Whether a value calls a function we cannot evaluate.
fn uses_an_unknown_function(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'(' {
            continue;
        }
        // Walk back over the function name preceding the parenthesis.
        let mut start = i;
        while start > 0 {
            let c = bytes[start - 1];
            if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' {
                start -= 1;
            } else {
                break;
            }
        }
        // `(` with nothing before it is a grouping parenthesis, not a call.
        if start == i {
            continue;
        }
        if !SUPPORTED_VALUE_FUNCTIONS.contains(&&lower[start..i]) {
            return true;
        }
    }
    false
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
            zoom: 1.0,
        }
    }
}

/// Evaluate a `calc()` expression to pixels.
///
/// Handles `+`, `-`, `*`, `/`, nested parentheses and any unit
/// [`parse_length_ctx`] understands. Percentages are rejected: resolving them
/// needs a containing block that is not known during style computation, which
/// is the same reason a bare `50%` is not a plain length here.
///
/// `s` is the text inside `calc(` … `)`.
fn eval_calc(s: &str, ctx: LengthContext) -> Option<f32> {
    // Tokenise into numbers-with-units, operators and parentheses. CSS requires
    // whitespace around + and - (so `10px -5px` is two values, not a
    // subtraction), and that also keeps signed exponents unambiguous.
    #[derive(Debug, PartialEq)]
    enum Tok {
        Num(f32),
        Op(char),
        Open,
        Close,
    }

    let mut toks = Vec::new();
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            toks.push(Tok::Open);
            i += 1;
        } else if c == ')' {
            toks.push(Tok::Close);
            i += 1;
        } else if matches!(c, '+' | '-' | '*' | '/') {
            toks.push(Tok::Op(c));
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                i += 1;
            }
            // Consume the unit, if any.
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == '%' {
                return None;
            }
            let lit: String = bytes[start..i].iter().collect();
            toks.push(Tok::Num(parse_length_ctx(&lit, ctx)?));
        } else {
            // Anything else (a nested function, a keyword) is unsupported.
            return None;
        }
    }
    if toks.is_empty() {
        return None;
    }

    // Recursive-descent over the token slice: expr → term (('+'|'-') term)*,
    // term → factor (('*'|'/') factor)*, factor → number | '(' expr ')' | ±factor.
    fn expr(toks: &[Tok], pos: &mut usize) -> Option<f32> {
        let mut acc = term(toks, pos)?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = toks.get(*pos) {
            let op = *op;
            *pos += 1;
            let rhs = term(toks, pos)?;
            acc = if op == '+' { acc + rhs } else { acc - rhs };
        }
        Some(acc)
    }

    fn term(toks: &[Tok], pos: &mut usize) -> Option<f32> {
        let mut acc = factor(toks, pos)?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = toks.get(*pos) {
            let op = *op;
            *pos += 1;
            let rhs = factor(toks, pos)?;
            if op == '*' {
                acc *= rhs;
            } else {
                if rhs == 0.0 {
                    return None;
                }
                acc /= rhs;
            }
        }
        Some(acc)
    }

    fn factor(toks: &[Tok], pos: &mut usize) -> Option<f32> {
        match toks.get(*pos)? {
            Tok::Num(n) => {
                let n = *n;
                *pos += 1;
                Some(n)
            }
            Tok::Open => {
                *pos += 1;
                let v = expr(toks, pos)?;
                match toks.get(*pos) {
                    Some(Tok::Close) => {
                        *pos += 1;
                        Some(v)
                    }
                    _ => None,
                }
            }
            Tok::Op(op @ ('+' | '-')) => {
                let neg = *op == '-';
                *pos += 1;
                let v = factor(toks, pos)?;
                Some(if neg { -v } else { v })
            }
            _ => None,
        }
    }

    let mut pos = 0;
    let value = expr(&toks, &mut pos)?;
    // Trailing tokens mean the expression was malformed.
    if pos != toks.len() || !value.is_finite() {
        return None;
    }
    Some(value)
}

/// Split a shorthand value on whitespace, keeping parenthesised functions whole.
///
/// `margin: calc(4px * 2) 10px` is two components, not four — a plain
/// `split_whitespace` would tear the `calc()` apart.
fn split_components(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;

    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                start.get_or_insert(idx);
            }
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    parts.push(&s[st..idx]);
                }
            }
            _ => {
                start.get_or_insert(idx);
            }
        }
    }
    if let Some(st) = start {
        parts.push(&s[st..]);
    }
    parts
}

/// Strip a wrapping `name(` … `)` from `s`, returning the contents.
fn strip_function<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(name)?;
    let rest = rest.strip_prefix('(')?;
    rest.strip_suffix(')')
}

/// Which CSS comparison function is being evaluated.
#[derive(Debug, Clone, Copy)]
enum MathFn {
    Min,
    Max,
    Clamp,
}

/// Evaluate `min()`, `max()` or `clamp()` over comma-separated length expressions.
fn eval_math_fn(inner: &str, which: MathFn, ctx: LengthContext) -> Option<f32> {
    // Split on top-level commas so nested calc(a, …) style parens stay intact.
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                args.push(&inner[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    args.push(&inner[start..]);

    let values: Option<Vec<f32>> = args
        .iter()
        .map(|a| {
            let a = a.trim();
            // A bare arithmetic expression is allowed inside these functions.
            parse_length_ctx(a, ctx).or_else(|| eval_calc(a, ctx))
        })
        .collect();
    let values = values?;
    if values.is_empty() {
        return None;
    }

    match which {
        MathFn::Min => Some(values.iter().copied().fold(f32::INFINITY, f32::min)),
        MathFn::Max => Some(values.iter().copied().fold(f32::NEG_INFINITY, f32::max)),
        MathFn::Clamp => {
            // clamp(min, preferred, max)
            let [lo, val, hi] = values[..] else {
                return None;
            };
            Some(val.clamp(lo, hi.max(lo)))
        }
    }
}

/// Parse a CSS length value into pixels, resolving relative units via `ctx`.
///
/// Supports absolute units (`px`, `pt`, `pc`, `in`, `cm`, `mm`, `q`), font-relative
/// units (`em`, `rem`, `ex`, `ch`), viewport units (`vw`, `vh`, `vmin`, `vmax`),
/// and `calc()` / `min()` / `max()` / `clamp()` over those.
/// A bare number is treated as pixels (quirks-friendly, and correct for `0`).
/// Returns `None` for `"auto"`, `"inherit"`, percentages, or unparseable values.
pub(crate) fn parse_length_ctx(s: &str, ctx: LengthContext) -> Option<f32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("inherit") {
        return None;
    }

    // Math functions. The comparison functions take a comma-separated list of
    // expressions, each of which may itself be arithmetic.
    let lower = s.to_ascii_lowercase();
    if let Some(inner) = strip_function(&lower, "calc") {
        return eval_calc(inner, ctx);
    }
    for (name, pick) in [
        ("min", MathFn::Min),
        ("max", MathFn::Max),
        ("clamp", MathFn::Clamp),
    ] {
        if let Some(inner) = strip_function(&lower, name) {
            return eval_math_fn(inner, pick, ctx);
        }
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
    // Page zoom scales absolute lengths only. Font-relative units resolve
    // against font sizes that are themselves already zoomed, and viewport units
    // resolve against the window, which zoom does not resize — scaling either
    // again would apply the zoom twice.
    let zoom = ctx.zoom;
    let px = match unit.as_str() {
        "" | "px" => num * zoom,
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
        "pt" => num * 96.0 / 72.0 * zoom,
        "pc" => num * 16.0 * zoom,
        "in" => num * 96.0 * zoom,
        "cm" => num * 96.0 / 2.54 * zoom,
        "mm" => num * 96.0 / 25.4 * zoom,
        "q" => num * 96.0 / 101.6 * zoom,
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
    let tokens: Vec<&str> = split_components(s);
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

/// The family used when an element has no usable `font-family`.
pub const DEFAULT_FONT_FAMILY: &str = "sans-serif";

/// The CSS generic families, which terminate a fallback chain.
const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "emoji",
    "math",
    "fangsong",
];

/// Normalise a `font-family` declaration into a fallback chain the text stack
/// can consume.
///
/// The value is a *prioritised list*, not one name: `font-family: "Segoe UI",
/// Roboto, sans-serif` means "try each in turn". Each entry is unquoted
/// individually — stripping quotes from the whole declaration would leave a
/// stray `"` glued to the second family and make it unmatchable.
///
/// A generic family is appended when the author did not end the list with one,
/// so a page naming only fonts this machine lacks still resolves to something
/// rather than to nothing.
pub fn normalize_font_family(val: &str) -> String {
    let mut families: Vec<String> = Vec::new();
    let mut has_generic = false;

    for raw in split_top_level_commas(val) {
        let name = raw.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if name.is_empty() || name.eq_ignore_ascii_case("inherit") {
            continue;
        }
        // `-apple-system` and friends are vendor aliases for the platform UI
        // font; they are unmatchable names here, so let them fall through to
        // the generic that follows them in the list.
        if name.starts_with('-') {
            continue;
        }
        if GENERIC_FAMILIES
            .iter()
            .any(|g| name.eq_ignore_ascii_case(g))
        {
            has_generic = true;
        }
        families.push(name.to_string());
    }

    if families.is_empty() {
        return String::new();
    }
    if !has_generic {
        families.push(DEFAULT_FONT_FAMILY.to_string());
    }
    families.join(", ")
}

/// Resolve a computed family for the text stack, substituting the default when
/// the cascade left it unset.
pub fn font_stack_or_default(family: &str) -> &str {
    if family.trim().is_empty() {
        DEFAULT_FONT_FAMILY
    } else {
        family
    }
}

/// Split a comma-separated list, ignoring commas nested inside `(...)`
/// (e.g. inside a `local(...)` or `format(...)` argument).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Parse a box model shorthand (margin/padding) into four values.
///
/// Supports 1, 2, 3, or 4 space-separated values:
/// - 1 value: all sides
/// - 2 values: vertical, horizontal
/// - 3 values: top, horizontal, bottom
/// - 4 values: top, right, bottom, left
fn parse_box_four(s: &str, fallback: [f32; 4], ctx: LengthContext) -> [f32; 4] {
    let parts: Vec<f32> = split_components(s)
        .into_iter()
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

/// The same 1/2/3/4-value box expansion as [`parse_box_four`], for any value
/// type — `border-color` and `border-style` take the identical shorthand form.
fn parse_box_four_generic<T: Copy>(
    s: &str,
    fallback: [T; 4],
    parse_one: impl Fn(&str) -> Option<T>,
) -> [T; 4] {
    let parts: Vec<T> = split_components(s)
        .into_iter()
        .filter_map(|p| parse_one(p.trim()))
        .collect();

    match parts.len() {
        1 => [parts[0]; 4],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        4 => [parts[0], parts[1], parts[2], parts[3]],
        _ => fallback,
    }
}

/// Map `border-<side>-*` to its index in a `[top, right, bottom, left]` array.
fn border_side_index(property: &str) -> usize {
    if property.contains("-right-") {
        1
    } else if property.contains("-bottom-") {
        2
    } else if property.contains("-left-") {
        3
    } else {
        0
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
    let parts: Vec<&str> = split_components(val.trim());

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
        (
            "ul",
            "display: block; padding-left: 40px; margin: 1em 0; list-style-type: disc",
        ),
        (
            "ol",
            "display: block; padding-left: 40px; margin: 1em 0; list-style-type: decimal",
        ),
        // Nested bullets change shape, which is how a reader tells one level
        // from the next when the indent alone is not enough.
        ("ul ul", "list-style-type: circle"),
        ("ul ul ul", "list-style-type: square"),
        ("menu", "display: block; padding-left: 40px; margin: 1em 0"),
        ("li", "display: list-item"),
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
        // Media elements are replaced content: a box of their own with chrome
        // painted into it, never a container the page flows through.
        ("video", "display: inline-block; background-color: #000000"),
        ("audio", "display: inline-block"),
        ("canvas", "display: inline-block"),
        // Embedded documents. The border is the one browsers have always drawn
        // round an iframe, and is what makes the boundary between the two
        // documents visible.
        (
            "iframe",
            "display: inline-block; border: 2px solid #c0c0c0; background-color: #ffffff",
        ),
        ("object", "display: inline-block"),
        ("embed", "display: inline-block"),
        ("source", "display: none"),
        ("track", "display: none"),
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
        // The extra right padding is room for the drop arrow the renderer draws.
        (
            "select",
            "display: inline-block; padding: 4px 22px 4px 8px; border: 1px solid #767676; border-radius: 2px; background-color: #ffffff; color: #000000",
        ),
        // Options are the select's data, not flow content. Without this every
        // option's text pours into the page around the control.
        ("option", "display: none"),
        ("optgroup", "display: none"),
        ("label", "display: inline"),
        ("head", "display: none"),
        ("script", "display: none"),
        ("style", "display: none"),
        ("meta", "display: none"),
        ("title", "display: none"),
        ("link", "display: none"),
        ("noscript", "display: none"),
        // `dir` is how documents actually set direction. Mapping it here rather
        // than reading the attribute during the cascade keeps it a declaration,
        // so it survives the inheritance rebuild and an author rule can still
        // override it — which is exactly what a real UA stylesheet does.
        ("[dir=\"rtl\"]", "direction: rtl"),
        ("[dir=\"ltr\"]", "direction: ltr"),
        ("[dir=rtl]", "direction: rtl"),
        ("[dir=ltr]", "direction: ltr"),
    ];

    let mut rules = Vec::new();

    for (order, (selector_str, decls_str)) in ua_rules.into_iter().enumerate() {
        let selectors = parser::parse_selector_str(selector_str);
        let declarations = parse_declarations(decls_str);
        rules.push(parser::CSSRule {
            selectors,
            declarations,
            order,
        });
    }

    parser::Stylesheet {
        rules,
        imports: Vec::new(),
        media_rules: Vec::new(),
        font_faces: Vec::new(),
        keyframes: Vec::new(),
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
    // The author's numbering restarts at zero, so it is slid past the UA
    // sheet's — otherwise sorting by source order would interleave the two and
    // let a UA rule outrank the page's own.
    let base = ua.rules.iter().map(|r| r.order + 1).max().unwrap_or(0);
    let shifted = |rule: &parser::CSSRule| parser::CSSRule {
        order: rule.order + base,
        ..rule.clone()
    };

    let mut rules = Vec::new();
    rules.extend_from_slice(&ua.rules);
    rules.extend(author.rules.iter().map(shifted));
    // UA stylesheet has no imports; pass through author imports (already resolved at fetch time)
    let imports = author.imports.clone();
    let media_rules = author
        .media_rules
        .iter()
        .map(|media| parser::MediaRule {
            condition: media.condition.clone(),
            rules: media.rules.iter().map(shifted).collect(),
        })
        .collect();
    parser::Stylesheet {
        rules,
        imports,
        media_rules,
        // The UA sheet declares neither web fonts nor animations.
        font_faces: author.font_faces.clone(),
        keyframes: author.keyframes.clone(),
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    fn transform(value: &str) -> Transform {
        Transform::parse(value, LengthContext::default())
    }

    /// The matrix a value comes to for a 100x50 box, about its default centre.
    fn matrix(value: &str) -> Matrix {
        transform(value).resolve(100.0, 50.0, TransformOrigin::default())
    }

    #[test]
    fn none_and_nonsense_leave_the_box_alone() {
        assert!(transform("none").is_none());
        assert!(transform("").is_none());
        assert!(transform("wobble(3)").is_none());
        assert!(matrix("none").is_identity());
    }

    #[test]
    fn a_translation_moves_the_box_by_the_length_given() {
        let m = matrix("translate(10px, -4px)");
        assert_eq!(m.apply(0.0, 0.0), (10.0, -4.0));
        assert_eq!(matrix("translateX(7px)").apply(0.0, 0.0), (7.0, 0.0));
        assert_eq!(matrix("translateY(7px)").apply(0.0, 0.0), (0.0, 7.0));
    }

    #[test]
    fn a_percentage_translation_is_a_fraction_of_the_box_itself() {
        // `translate(-50%, -50%)` is how a page centres something, and it can
        // only be resolved once the box has a size.
        let m = matrix("translate(-50%, -50%)");
        assert_eq!(m.apply(0.0, 0.0), (-50.0, -25.0));
    }

    #[test]
    fn a_single_scale_argument_scales_both_axes() {
        let m = matrix("scale(2)");
        assert_eq!((m.a, m.d), (2.0, 2.0));
        let m = matrix("scale(2, 3)");
        assert_eq!((m.a, m.d), (2.0, 3.0));
    }

    #[test]
    fn scaling_happens_about_the_centre_of_the_box_by_default() {
        // The centre stays put and the edges move outwards, which is what
        // makes a hover-grow effect grow in place.
        let m = matrix("scale(2)");
        assert_eq!(m.apply(50.0, 25.0), (50.0, 25.0), "the centre is fixed");
        assert_eq!(m.apply(0.0, 0.0), (-50.0, -25.0), "the corner moves out");
    }

    #[test]
    fn transform_origin_moves_the_point_it_all_happens_about() {
        let origin = TransformOrigin {
            x: LengthOrPercent::default(),
            y: LengthOrPercent::default(),
        };
        let m = transform("scale(2)").resolve(100.0, 50.0, origin);
        assert_eq!(m.apply(0.0, 0.0), (0.0, 0.0), "the top left is fixed now");
        assert_eq!(m.apply(100.0, 50.0), (200.0, 100.0));
    }

    #[test]
    fn transform_origin_keywords_name_the_corners() {
        let ctx = LengthContext::default();
        let top_left = TransformOrigin::parse("left top", ctx);
        assert_eq!(top_left.x.percent, 0.0);
        assert_eq!(top_left.y.percent, 0.0);

        let bottom_right = TransformOrigin::parse("right bottom", ctx);
        assert_eq!(bottom_right.x.percent, 1.0);
        assert_eq!(bottom_right.y.percent, 1.0);

        let default = TransformOrigin::default();
        assert_eq!((default.x.percent, default.y.percent), (0.5, 0.5));
    }

    #[test]
    fn a_list_of_functions_applies_right_to_left() {
        // `translate(10px) scale(2)` scales first and then moves, so the point
        // at the box's centre lands 10px along rather than 20.
        let m = transform("translate(10px, 0) scale(2)").resolve(
            100.0,
            50.0,
            TransformOrigin {
                x: LengthOrPercent::default(),
                y: LengthOrPercent::default(),
            },
        );
        assert_eq!(m.apply(0.0, 0.0), (10.0, 0.0));
        assert_eq!(m.apply(10.0, 0.0), (30.0, 0.0));
    }

    #[test]
    fn a_rotation_is_parsed_in_whatever_unit_it_is_written() {
        let quarter = std::f32::consts::FRAC_PI_2;
        for value in ["rotate(90deg)", "rotate(1.5708rad)", "rotate(0.25turn)"] {
            let m = matrix(value);
            assert!(
                (m.b - quarter.sin()).abs() < 1e-3,
                "{value} should be a quarter turn, got {m:?}"
            );
        }
    }

    #[test]
    fn a_rotation_is_recognised_as_something_the_painter_cannot_do() {
        // The compositor draws axis-aligned rectangles and upright glyphs.
        assert!(!matrix("rotate(45deg)").is_axis_aligned());
        assert!(matrix("scale(2) translate(4px)").is_axis_aligned());
    }

    #[test]
    fn a_matrix_is_taken_as_written() {
        let m = matrix("matrix(2, 0, 0, 3, 10, 20)");
        assert_eq!((m.a, m.b, m.c, m.d), (2.0, 0.0, 0.0, 3.0));
        // The origin bracketing still applies, so the centre stays put.
        assert_eq!(m.apply(50.0, 25.0), (50.0 + 10.0, 25.0 + 20.0));
    }

    #[test]
    fn the_property_reaches_computed_values() {
        let computed = ComputedValues::default().from_declaration(&Declaration {
            property: "transform".to_string(),
            value: "translate(5px, 6px)".to_string(),
            important: false,
        });
        assert!(!computed.transform.is_none());
        assert_eq!(
            computed
                .transform
                .resolve(10.0, 10.0, TransformOrigin::default())
                .apply(0.0, 0.0),
            (5.0, 6.0)
        );
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

    // ------ Cascade source order ------

    /// The colour the first `<a>` ends up with, from author CSS alone.
    fn cascaded_link_color(css: &str) -> Option<[u8; 4]> {
        let arena = crate::html::parse_html("<a>link</a>");
        let author = parser::parse_stylesheet(css);
        let merged = merge_stylesheets_with_author(&user_agent_stylesheet(), &author);
        let styles = compute_styles_for_tree(&arena, &merged, (1280.0, 800.0));
        styles_for_tag(&arena, &styles, "a").color
    }

    /// At equal specificity the later rule wins — including when the earlier
    /// one is inside a `@media` block. Collecting the media rules after all the
    /// plain ones used to hand them the tie regardless of where they were
    /// written.
    #[test]
    fn a_later_rule_beats_an_earlier_media_rule() {
        assert_eq!(
            cascaded_link_color("@media screen { a { color: red } } a { color: lime }"),
            Some([0, 255, 0, 255]),
            "the unconditional rule comes second and wins"
        );
    }

    #[test]
    fn a_later_media_rule_beats_an_earlier_plain_rule() {
        assert_eq!(
            cascaded_link_color("a { color: red } @media screen { a { color: lime } }"),
            Some([0, 255, 0, 255])
        );
    }

    /// The author's rules restart their numbering at zero, so they have to be
    /// slid past the UA sheet or the two interleave.
    #[test]
    fn an_author_rule_beats_the_ua_sheet_at_equal_specificity() {
        assert_eq!(
            cascaded_link_color("a { color: red }"),
            Some([255, 0, 0, 255]),
            "the UA sheet also has a plain `a` rule, and must lose to this one"
        );
    }

    #[test]
    fn a_supports_block_keeps_the_position_it_was_written_at() {
        assert_eq!(
            cascaded_link_color("@supports (display:grid) { a { color: red } } a { color: lime }"),
            Some([0, 255, 0, 255])
        );
        assert_eq!(
            cascaded_link_color("a { color: lime } @supports (display:grid) { a { color: red } }"),
            Some([255, 0, 0, 255])
        );
    }

    // ------ `:not()`, `:is()`, `:where()` ------

    /// Whether an author rule painting `<a>` red reached the first one.
    ///
    /// The UA sheet already gives a link a colour, so "did the rule apply" is
    /// the question, not "is a colour set".
    fn link_is_red(css: &str, html: &str) -> bool {
        let arena = crate::html::parse_html(html);
        let author = parser::parse_stylesheet(css);
        let merged = merge_stylesheets_with_author(&user_agent_stylesheet(), &author);
        let styles = compute_styles_for_tree(&arena, &merged, (1280.0, 800.0));
        styles_for_tag(&arena, &styles, "a").color == Some([255, 0, 0, 255])
    }

    /// Every `:not()` used to fail to match, so 360 of the rules on
    /// ja.wikipedia.org never reached an element.
    #[test]
    fn not_matches_when_its_argument_does_not() {
        assert!(
            link_is_red("a:not(.notheme) { color: red }", "<a>link</a>"),
            "the element has no class, so :not(.notheme) holds"
        );
        assert!(
            !link_is_red(
                "a:not(.notheme) { color: red }",
                r#"<a class="notheme">link</a>"#
            ),
            "the element has the class, so the rule must not apply"
        );
    }

    #[test]
    fn not_takes_a_selector_list() {
        assert!(link_is_red(
            "a:not(.x, .y) { color: red }",
            r#"<a class="z">l</a>"#
        ));
        assert!(
            !link_is_red("a:not(.x, .y) { color: red }", r#"<a class="y">l</a>"#),
            "matching either arm of the list rules the element out"
        );
    }

    #[test]
    fn not_takes_an_attribute_selector() {
        assert!(link_is_red(
            r#"a:not([role="button"]) { color: red }"#,
            r#"<a href="/x">l</a>"#
        ));
        assert!(!link_is_red(
            r#"a:not([role="button"]) { color: red }"#,
            r#"<a role="button">l</a>"#
        ));
    }

    /// An unknown pseudo-class inside `:not()` cannot be shown to match, and a
    /// static element is not `:active` or `:focus` anyway.
    #[test]
    fn not_of_a_state_pseudo_class_holds_for_a_resting_element() {
        assert!(link_is_red("a:not(:active) { color: red }", "<a>l</a>"));
    }

    #[test]
    fn is_and_where_match_any_of_their_arguments() {
        assert!(link_is_red(
            "a:is(.x, .y) { color: red }",
            r#"<a class="y">l</a>"#
        ));
        assert!(!link_is_red(
            "a:is(.x, .y) { color: red }",
            r#"<a class="z">l</a>"#
        ));
        assert!(link_is_red(
            "a:where(.x) { color: red }",
            r#"<a class="x">l</a>"#
        ));
    }

    #[test]
    fn nested_functions_survive_being_read_back() {
        // The argument is rebuilt as text and parsed again; a nested `:not()`
        // used to be cut off at the inner closing parenthesis.
        assert!(link_is_red(
            r#"a:where(.new:not([role="button"])) { color: red }"#,
            r#"<a class="new">l</a>"#
        ));
        assert!(!link_is_red(
            r#"a:where(.new:not([role="button"])) { color: red }"#,
            r#"<a class="new" role="button">l</a>"#
        ));
    }

    #[test]
    fn where_contributes_no_specificity_and_not_contributes_its_argument() {
        let sel = |src: &str| parser::parse_selector_str(src)[0].specificity();
        assert_eq!(sel("a:where(.x)"), (0, 0, 1), ":where() adds nothing");
        assert_eq!(sel("a:not(.x)"), (0, 1, 1), ":not() counts as its argument");
        assert_eq!(sel("a:not(#x)"), (1, 0, 1));
        assert_eq!(
            sel("a:is(#x, .y)"),
            (1, 0, 1),
            ":is() takes the most specific"
        );
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

    // ------ calc() and math functions ------

    #[test]
    fn calc_does_basic_arithmetic() {
        let ctx = LengthContext::default();
        assert_eq!(parse_length_ctx("calc(100px + 20px)", ctx), Some(120.0));
        assert_eq!(parse_length_ctx("calc(100px - 20px)", ctx), Some(80.0));
        assert_eq!(parse_length_ctx("calc(100px * 2)", ctx), Some(200.0));
        assert_eq!(parse_length_ctx("calc(100px / 4)", ctx), Some(25.0));
    }

    #[test]
    fn calc_respects_precedence_and_parentheses() {
        let ctx = LengthContext::default();
        assert_eq!(parse_length_ctx("calc(10px + 2 * 20px)", ctx), Some(50.0));
        assert_eq!(parse_length_ctx("calc((10px + 20px) * 2)", ctx), Some(60.0));
        assert_eq!(
            parse_length_ctx("calc(100px - (20px + 30px))", ctx),
            Some(50.0)
        );
    }

    #[test]
    fn calc_mixes_units_through_the_context() {
        let ctx = LengthContext {
            font_size: 10.0,
            root_font_size: 20.0,
            viewport_width: 1000.0,
            viewport_height: 500.0,
            ..LengthContext::default()
        };
        assert_eq!(parse_length_ctx("calc(2em + 1rem)", ctx), Some(40.0));
        assert_eq!(parse_length_ctx("calc(10vw - 50px)", ctx), Some(50.0));
        assert_eq!(parse_length_ctx("CALC(1EM * 3)", ctx), Some(30.0));
    }

    #[test]
    fn calc_handles_unary_signs() {
        let ctx = LengthContext::default();
        assert_eq!(parse_length_ctx("calc(-10px + 30px)", ctx), Some(20.0));
        assert_eq!(parse_length_ctx("calc(10px * -2)", ctx), Some(-20.0));
    }

    #[test]
    fn calc_rejects_percentages_and_malformed_input() {
        let ctx = LengthContext::default();
        // Percentages need a containing block, which style computation lacks.
        assert_eq!(parse_length_ctx("calc(100% - 20px)", ctx), None);
        assert_eq!(parse_length_ctx("calc(10px +)", ctx), None);
        assert_eq!(parse_length_ctx("calc((10px)", ctx), None);
        assert_eq!(parse_length_ctx("calc(10px / 0)", ctx), None);
        assert_eq!(parse_length_ctx("calc()", ctx), None);
        assert_eq!(parse_length_ctx("calc(red)", ctx), None);
    }

    #[test]
    fn min_max_clamp_pick_the_right_value() {
        let ctx = LengthContext::default();
        assert_eq!(parse_length_ctx("min(10px, 40px, 20px)", ctx), Some(10.0));
        assert_eq!(parse_length_ctx("max(10px, 40px, 20px)", ctx), Some(40.0));
        assert_eq!(parse_length_ctx("clamp(10px, 5px, 40px)", ctx), Some(10.0));
        assert_eq!(parse_length_ctx("clamp(10px, 25px, 40px)", ctx), Some(25.0));
        assert_eq!(parse_length_ctx("clamp(10px, 90px, 40px)", ctx), Some(40.0));
    }

    #[test]
    fn math_functions_accept_nested_arithmetic() {
        let ctx = LengthContext {
            font_size: 10.0,
            ..LengthContext::default()
        };
        assert_eq!(
            parse_length_ctx("max(calc(2em + 5px), 20px)", ctx),
            Some(25.0)
        );
        assert_eq!(parse_length_ctx("min(3em, 40px)", ctx), Some(30.0));
    }

    #[test]
    fn calc_works_end_to_end_in_the_cascade() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>x</div></body></html>",
            "div { font-size: 10px; width: calc(100px + 2em); padding: calc(4px * 2); }",
        );

        let div = styles_for_tag(&arena, &styles, "div");
        assert_eq!(div.width, Some(120.0));
        assert_eq!(div.padding, [8.0; 4]);
    }

    #[test]
    fn z_index_parses_integers_and_auto() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>x</div></body></html>",
            "div { z-index: 5; }",
        );
        assert_eq!(styles_for_tag(&arena, &styles, "div").z_index, Some(5));

        let (arena, styles) = styles_with_ua(
            "<html><body><div>x</div></body></html>",
            "div { z-index: -2; }",
        );
        assert_eq!(styles_for_tag(&arena, &styles, "div").z_index, Some(-2));

        let (arena, styles) = styles_with_ua(
            "<html><body><div>x</div></body></html>",
            "div { z-index: auto; }",
        );
        assert_eq!(styles_for_tag(&arena, &styles, "div").z_index, None);
    }

    #[test]
    fn z_index_does_not_inherit() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div><span>x</span></div></body></html>",
            "div { z-index: 7; }",
        );
        assert_eq!(styles_for_tag(&arena, &styles, "div").z_index, Some(7));
        assert_eq!(styles_for_tag(&arena, &styles, "span").z_index, None);
    }

    #[test]
    fn shorthand_splitting_keeps_functions_whole() {
        assert_eq!(split_components("10px 20px"), vec!["10px", "20px"]);
        assert_eq!(
            split_components("calc(4px * 2) 10px"),
            vec!["calc(4px * 2)", "10px"]
        );
        assert_eq!(
            split_components("  min(1em, 4px)   auto "),
            vec!["min(1em, 4px)", "auto"]
        );
        assert!(split_components("   ").is_empty());
    }

    #[test]
    fn calc_in_a_multi_value_shorthand() {
        let (arena, styles) = styles_with_ua(
            "<html><body><div>x</div></body></html>",
            "div { font-size: 10px; margin: calc(1em + 2px) 5px; }",
        );

        // top/bottom from the calc, left/right from the literal.
        assert_eq!(
            styles_for_tag(&arena, &styles, "div").margin,
            [12.0, 5.0, 12.0, 5.0]
        );
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

    // -- Pseudo-class selection --

    /// A colour no user-agent rule produces, so a match is unambiguous.
    ///
    /// Set as a background rather than a text colour: `color` is inherited, so
    /// a rule that picks out one element would appear to have picked out
    /// everything inside it too.
    const MARK: [u8; 4] = [1, 2, 3, 255];

    /// The `id`s of the elements a rule picks out, in document order.
    ///
    /// The rule under test sets one distinctive colour; everything else about
    /// the cascade is beside the point here, so the assertion reads as the list
    /// of elements the selector chose.
    fn selected(css_text: &str, html_text: &str) -> Vec<String> {
        selected_with_state(css_text, html_text, &InteractionState::default())
    }

    /// The same, with the page in a given pointer and keyboard state.
    fn selected_with_state(
        css_text: &str,
        html_text: &str,
        state: &InteractionState<'_>,
    ) -> Vec<String> {
        let arena = crate::html::parse_html(html_text);
        let stylesheet = merge_stylesheets_with_author(
            &user_agent_stylesheet(),
            &parser::parse_stylesheet(css_text),
        );
        let styles =
            compute_styles_for_tree_with_state(&arena, &stylesheet, (1024.0, 768.0), state, 1.0);

        let nodes = arena.nodes.borrow();
        nodes
            .iter()
            .enumerate()
            .filter(|(i, node)| {
                node.is_element()
                    && styles.get(&(*i as u32)).and_then(|c| c.background_color) == Some(MARK)
            })
            .filter_map(|(i, _)| {
                arena
                    .get(crate::html::DomHandle(crate::html::NodeId::from_raw(
                        i as u32,
                    )))
                    .and_then(|n| n.get_attr("id").map(|id| id.to_string()))
            })
            .collect()
    }

    /// The node id of the element carrying this `id` attribute.
    fn node_with_id(arena: &crate::html::DomArena, wanted: &str) -> u32 {
        let nodes = arena.nodes.borrow();
        (0..nodes.len() as u32)
            .find(|&i| {
                arena
                    .get(crate::html::DomHandle(crate::html::NodeId::from_raw(i)))
                    .and_then(|n| n.get_attr("id").map(|id| id.to_string()))
                    .is_some_and(|id| id == wanted)
            })
            .expect("no element with that id")
    }

    /// A list whose items differ in tag, so of-type and nth-child disagree.
    const MIXED: &str = r#"<html><body><div id="wrap">
        <p id="p1">one</p>
        <span id="s1">two</span>
        <p id="p2">three</p>
        <span id="s2">four</span>
        <p id="p3">five</p>
    </div></body></html>"#;

    #[test]
    fn root_selects_the_element_the_document_hangs_off() {
        let styles_for = |css: &str| {
            selected(
                css,
                r#"<html id="doc"><body id="body"><div id="d">x</div></body></html>"#,
            )
        };
        assert_eq!(
            styles_for(":root { background-color: rgb(1,2,3) }"),
            ["doc"]
        );
        assert_eq!(
            styles_for(":root div { background-color: rgb(1,2,3) }"),
            ["d"],
            ":root is an ancestor like any other, not only a rule of its own"
        );
    }

    #[test]
    fn empty_selects_elements_with_nothing_in_them() {
        assert_eq!(
            selected(
                ":empty { background-color: rgb(1,2,3) }",
                r#"<html><body>
                    <div id="hollow"></div>
                    <div id="spaces">   </div>
                    <div id="worded">x</div>
                    <div id="nested"><span id="inner"></span></div>
                </body></html>"#,
            ),
            ["hollow", "spaces", "inner"],
            "whitespace between tags is not content, but an element is"
        );
    }

    #[test]
    fn of_type_counts_only_the_siblings_that_share_a_tag() {
        assert_eq!(
            selected("p:first-of-type { background-color: rgb(1,2,3) }", MIXED),
            ["p1"]
        );
        assert_eq!(
            selected("p:last-of-type { background-color: rgb(1,2,3) }", MIXED),
            ["p3"]
        );
        assert_eq!(
            selected("p:nth-of-type(2) { background-color: rgb(1,2,3) }", MIXED),
            ["p2"],
            "the second p, which is the third child"
        );
        assert_eq!(
            selected("p:nth-child(2) { background-color: rgb(1,2,3) }", MIXED),
            Vec::<String>::new(),
            "the second child is a span, so no p matches"
        );
        assert_eq!(
            selected(
                "span:nth-last-of-type(1) { background-color: rgb(1,2,3) }",
                MIXED
            ),
            ["s2"]
        );
    }

    #[test]
    fn only_of_type_needs_to_be_the_one_and_only() {
        let page = r#"<html><body><div id="wrap">
            <h1 id="h">title</h1>
            <p id="a">one</p>
            <p id="b">two</p>
        </div></body></html>"#;
        assert_eq!(
            selected("h1:only-of-type { background-color: rgb(1,2,3) }", page),
            ["h"]
        );
        assert_eq!(
            selected("p:only-of-type { background-color: rgb(1,2,3) }", page),
            Vec::<String>::new()
        );
    }

    #[test]
    fn nth_last_child_counts_from_the_end() {
        assert_eq!(
            selected(
                "#wrap > :nth-last-child(2) { background-color: rgb(1,2,3) }",
                MIXED
            ),
            ["s2"]
        );
        assert_eq!(
            selected(
                "#wrap > :last-child { background-color: rgb(1,2,3) }",
                MIXED
            ),
            ["p3"]
        );
    }

    /// An element whose position among its siblings was never worked out must
    /// not fall through to "first". Nothing positional should match it.
    #[test]
    fn an_element_with_no_known_position_matches_nothing_positional() {
        let facts = parser::ElementFacts::for_tag("li");
        for selector in [
            "li:first-child",
            "li:last-child",
            "li:only-child",
            "li:nth-child(1)",
            "li:first-of-type",
            "li:nth-of-type(1)",
        ] {
            let parsed = parser::parse_stylesheet(&format!("{selector} {{ color: red }}"));
            assert!(
                !parsed.rules[0].selectors[0].matches_facts(&facts),
                "{selector} matched an element with no position"
            );
        }
    }

    #[test]
    fn checked_follows_the_markup() {
        assert_eq!(
            selected(
                ":checked { background-color: rgb(1,2,3) }",
                r#"<html><body>
                    <input id="on" type="checkbox" checked>
                    <input id="off" type="checkbox">
                </body></html>"#,
            ),
            ["on"]
        );
    }

    #[test]
    fn disabled_and_enabled_are_both_only_about_controls() {
        let page = r#"<html><body>
            <input id="live">
            <input id="dead" disabled>
            <div id="neither">text</div>
        </body></html>"#;
        assert_eq!(
            selected(":disabled { background-color: rgb(1,2,3) }", page),
            ["dead"]
        );
        assert_eq!(
            selected(":enabled { background-color: rgb(1,2,3) }", page),
            ["live"],
            "a div is neither enabled nor disabled — it is not a control"
        );
    }

    #[test]
    fn a_link_is_unvisited_because_we_keep_no_history() {
        let page = r#"<html><body>
            <a id="linked" href="/somewhere">go</a>
            <a id="anchor">no href</a>
        </body></html>"#;
        assert_eq!(
            selected("a:link { background-color: rgb(1,2,3) }", page),
            ["linked"]
        );
        assert_eq!(
            selected("a:any-link { background-color: rgb(1,2,3) }", page),
            ["linked"]
        );
        assert_eq!(
            selected("a:visited { background-color: rgb(1,2,3) }", page),
            Vec::<String>::new(),
            "with no history nothing has been visited, and the unvisited colour shows"
        );
    }

    #[test]
    fn focus_selects_the_element_holding_it() {
        let page = r#"<html><body>
            <input id="one">
            <input id="two">
        </body></html>"#;
        let arena = crate::html::parse_html(page);
        let two = node_with_id(&arena, "two");

        assert_eq!(
            selected(":focus { background-color: rgb(1,2,3) }", page),
            Vec::<String>::new(),
            "nothing is focused until something is"
        );
        assert_eq!(
            selected_with_state(
                ":focus { background-color: rgb(1,2,3) }",
                page,
                &InteractionState {
                    hovered: &[],
                    focused: Some(two),
                },
            ),
            ["two"]
        );
    }

    /// We have one way of focusing something, so the keyboard-only spelling of
    /// `:focus` says the same thing rather than nothing.
    #[test]
    fn focus_visible_answers_the_same_as_focus() {
        let page = r#"<html><body><input id="one"></body></html>"#;
        let arena = crate::html::parse_html(page);
        let one = node_with_id(&arena, "one");
        assert_eq!(
            selected_with_state(
                ":focus-visible { background-color: rgb(1,2,3) }",
                page,
                &InteractionState {
                    hovered: &[],
                    focused: Some(one),
                },
            ),
            ["one"]
        );
    }

    #[test]
    fn a_pseudo_class_we_cannot_answer_for_never_matches() {
        // Fail-closed: a rule guarded by something we do not implement should
        // not apply to everything, which is what a permissive default would do.
        assert_eq!(
            selected(
                "div:target { background-color: rgb(1,2,3) }",
                r#"<html><body><div id="d">x</div></body></html>"#,
            ),
            Vec::<String>::new()
        );
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

    /// Apply one `property: value` pair to a fresh `ComputedValues`.
    fn declare(property: &str, value: &str) -> ComputedValues {
        ComputedValues::default().from_declaration(&Declaration {
            property: property.to_string(),
            value: value.to_string(),
            important: false,
        })
    }

    #[test]
    fn background_image_url_is_extracted() {
        assert_eq!(
            declare("background-image", "url('images/hero.png')").background_image,
            Some("images/hero.png".to_string())
        );
        assert_eq!(
            declare("background-image", "URL(\"a.png\")").background_image,
            Some("a.png".to_string())
        );
        assert_eq!(declare("background-image", "none").background_image, None);
    }

    #[test]
    fn gradients_are_not_treated_as_fetchable_images() {
        assert_eq!(
            declare("background-image", "linear-gradient(red, blue)").background_image,
            None
        );
    }

    #[test]
    fn background_shorthand_reads_every_component() {
        let cv = declare(
            "background",
            "#ffffff url(bg.png) no-repeat center top / cover",
        );
        assert_eq!(cv.background_color, Some([255, 255, 255, 255]));
        assert_eq!(cv.background_image, Some("bg.png".to_string()));
        assert_eq!(cv.background_repeat, BackgroundRepeat::NoRepeat);
        assert_eq!(cv.background_size, BackgroundSize::Cover);
        assert_eq!(
            cv.background_position,
            BackgroundPosition {
                x: BackgroundLength::Percent(0.5),
                y: BackgroundLength::Percent(0.0)
            }
        );
    }

    #[test]
    fn background_shorthand_resets_an_image_from_an_earlier_rule() {
        let cv = declare("background-image", "url(old.png)").from_declaration(&Declaration {
            property: "background".to_string(),
            value: "#000".to_string(),
            important: false,
        });
        assert_eq!(cv.background_image, None);
        assert_eq!(cv.background_color, Some([0, 0, 0, 255]));
    }

    #[test]
    fn background_position_keywords_become_percentages() {
        let ctx = LengthContext::default();
        assert_eq!(
            BackgroundPosition::parse("right bottom", ctx).unwrap(),
            BackgroundPosition {
                x: BackgroundLength::Percent(1.0),
                y: BackgroundLength::Percent(1.0)
            }
        );
        // A single value sets x; y defaults to center.
        assert_eq!(
            BackgroundPosition::parse("left", ctx).unwrap(),
            BackgroundPosition {
                x: BackgroundLength::Percent(0.0),
                y: BackgroundLength::Percent(0.5)
            }
        );
        // Keywords may arrive in either order because each names its own axis.
        assert_eq!(
            BackgroundPosition::parse("top center", ctx).unwrap(),
            BackgroundPosition {
                x: BackgroundLength::Percent(0.5),
                y: BackgroundLength::Percent(0.0)
            }
        );
    }

    #[test]
    fn background_size_forms_parse() {
        let ctx = LengthContext::default();
        assert_eq!(
            parse_background_size("cover", ctx),
            Some(BackgroundSize::Cover)
        );
        assert_eq!(
            parse_background_size("50% auto", ctx),
            Some(BackgroundSize::Explicit(
                BackgroundLength::Percent(0.5),
                BackgroundLength::Auto
            ))
        );
        assert_eq!(
            parse_background_size("32px", ctx),
            Some(BackgroundSize::Explicit(
                BackgroundLength::Pixels(32.0),
                BackgroundLength::Auto
            ))
        );
    }

    #[test]
    fn background_repeat_two_value_form_names_each_axis() {
        assert_eq!(
            declare("background-repeat", "repeat no-repeat").background_repeat,
            BackgroundRepeat::RepeatX
        );
        assert_eq!(
            declare("background-repeat", "no-repeat repeat").background_repeat,
            BackgroundRepeat::RepeatY
        );
    }

    #[test]
    fn cursor_keywords_parse() {
        assert_eq!(declare("cursor", "pointer").cursor, Cursor::Pointer);
        assert_eq!(declare("cursor", "NOT-ALLOWED").cursor, Cursor::NotAllowed);
        assert_eq!(declare("cursor", "zoom-in").cursor, Cursor::ZoomIn);
        assert_eq!(declare("cursor", "none").cursor, Cursor::None);
    }

    #[test]
    fn cursor_falls_through_url_values_to_the_keyword() {
        // The fallback list exists precisely because a url() cursor may fail
        // to load; we never load them, so the keyword always wins.
        assert_eq!(
            declare("cursor", "url(grab.cur) 4 12, grab, pointer").cursor,
            Cursor::Grab
        );
    }

    #[test]
    fn cursor_keeps_its_value_when_a_later_declaration_is_junk() {
        let cv = declare("cursor", "wait").from_declaration(&Declaration {
            property: "cursor".to_string(),
            value: "nonsense-cursor".to_string(),
            important: false,
        });
        assert_eq!(cv.cursor, Cursor::Wait);
    }

    #[test]
    fn cursor_inherits_to_descendants() {
        let html = "<html><body><div id='outer'><span id='inner'>x</span></div></body></html>";
        let css = "#outer { cursor: pointer; }";
        let arena = crate::html::parse_html(html);
        let stylesheet = parser::parse_stylesheet(css);
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let inner_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("inner"))
            .unwrap() as u32;
        assert_eq!(styles.get(&inner_id).unwrap().cursor, Cursor::Pointer);
    }

    #[test]
    fn border_shorthand_reads_all_three_components_in_any_order() {
        let cv = declare("border", "dashed 2px #ff0000");
        assert_eq!(cv.border_style, [BorderStyle::Dashed; 4]);
        assert_eq!(cv.border_width, [2.0; 4]);
        assert_eq!(cv.border_color, [Some([255, 0, 0, 255]); 4]);
    }

    #[test]
    fn border_shorthand_without_a_width_uses_medium() {
        let cv = declare("border", "solid red");
        assert_eq!(cv.used_border_width(), [MEDIUM_BORDER_WIDTH; 4]);
        assert_eq!(cv.border_width, [MEDIUM_BORDER_WIDTH; 4]);
    }

    #[test]
    fn border_none_clears_an_earlier_border() {
        let cv = declare("border", "4px solid black").from_declaration(&Declaration {
            property: "border".to_string(),
            value: "none".to_string(),
            important: false,
        });
        assert_eq!(cv.used_border_width(), [0.0; 4]);
        assert!(!cv.has_visible_border());
    }

    #[test]
    fn border_width_without_a_style_occupies_no_space() {
        // CSS: `border-style` is initially `none`, which forces the used width
        // to zero however wide `border-width` says the side is.
        let cv = declare("border-width", "5px");
        assert_eq!(cv.border_width, [5.0; 4]);
        assert_eq!(cv.used_border_width(), [0.0; 4]);
        assert!(!cv.has_visible_border());
    }

    #[test]
    fn border_style_expands_like_any_box_shorthand() {
        let cv = declare("border-style", "solid dotted");
        assert_eq!(
            cv.border_style,
            [
                BorderStyle::Solid,
                BorderStyle::Dotted,
                BorderStyle::Solid,
                BorderStyle::Dotted
            ]
        );
    }

    #[test]
    fn per_side_borders_keep_their_own_colour_and_style() {
        let cv = declare("border-top", "1px solid red").from_declaration(&Declaration {
            property: "border-bottom".to_string(),
            value: "2px dotted blue".to_string(),
            important: false,
        });
        assert_eq!(cv.border_style[0], BorderStyle::Solid);
        assert_eq!(cv.border_style[2], BorderStyle::Dotted);
        assert_eq!(cv.border_color[0], Some([255, 0, 0, 255]));
        assert_eq!(cv.border_color[2], Some([0, 0, 255, 255]));
        assert_eq!(cv.used_border_width(), [1.0, 0.0, 2.0, 0.0]);
    }

    #[test]
    fn border_colour_defaults_to_current_color() {
        let cv = declare("color", "#00ff00").from_declaration(&Declaration {
            property: "border".to_string(),
            value: "1px solid".to_string(),
            important: false,
        });
        assert_eq!(cv.border_color[0], None, "currentColor stays unresolved");
        assert_eq!(cv.resolved_border_color(0), [0, 255, 0, 255]);
    }

    #[test]
    fn font_family_keeps_the_whole_fallback_chain() {
        let cv = declare("font-family", r#""Segoe UI", Roboto, sans-serif"#);
        assert_eq!(cv.font_family, "Segoe UI, Roboto, sans-serif");
    }

    #[test]
    fn font_family_unquotes_each_entry_not_just_the_ends() {
        // Trimming quotes from the whole declaration used to leave a stray
        // quote glued to the last family, making it unmatchable.
        assert_eq!(
            normalize_font_family(r#"'Hiragino Kaku Gothic ProN', "Meiryo", serif"#),
            "Hiragino Kaku Gothic ProN, Meiryo, serif"
        );
    }

    #[test]
    fn font_family_appends_a_generic_when_the_author_omitted_one() {
        assert_eq!(
            normalize_font_family("Impact, Charcoal"),
            "Impact, Charcoal, sans-serif"
        );
        // Already generic-terminated lists are left alone.
        assert_eq!(
            normalize_font_family("Courier New, monospace"),
            "Courier New, monospace"
        );
    }

    #[test]
    fn font_family_drops_vendor_prefixed_aliases() {
        assert_eq!(
            normalize_font_family("-apple-system, BlinkMacSystemFont, Arial, sans-serif"),
            "BlinkMacSystemFont, Arial, sans-serif"
        );
    }

    #[test]
    fn font_family_empty_declaration_leaves_the_cascade_alone() {
        let cv = declare("font-family", "monospace").from_declaration(&Declaration {
            property: "font-family".to_string(),
            value: "   ".to_string(),
            important: false,
        });
        assert_eq!(
            cv.font_family, "monospace",
            "a junk value must not clear it"
        );
        assert_eq!(font_stack_or_default(""), DEFAULT_FONT_FAMILY);
    }

    #[test]
    fn font_family_inherits_to_descendants() {
        let html = "<html><body><div id='outer'><span id='inner'>x</span></div></body></html>";
        let css = "#outer { font-family: 'Courier New', monospace; }";
        let arena = crate::html::parse_html(html);
        let stylesheet = parser::parse_stylesheet(css);
        let styles = compute_styles_for_tree(&arena, &stylesheet, (1024.0, 768.0));

        let nodes = arena.nodes.borrow();
        let inner_id = nodes
            .iter()
            .position(|n| n.is_element() && n.get_attr("id") == Some("inner"))
            .unwrap() as u32;

        assert_eq!(
            styles.get(&inner_id).unwrap().font_family,
            "Courier New, monospace"
        );
    }
}
