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

    // Named colors
    let named = parse_named_color(&lower);
    if let Some(_rgb) = named {
        return Some(CSSColor::Named(lower));
    }

    None
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
        ("transparent", (0, 0, 0)),
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
) -> FxHashMap<u32, ComputedValues> {
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
    drop(nodes_ref);

    // Phase 1: Cascade — for each element, find matching rules and apply
    for &node_id in &element_ids {
        let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(node_id));
        let node = match arena.get(handle) {
            Some(n) => n,
            None => continue,
        };

        let tag: String = match node.tag_name() {
            Some(t) => (*t).to_string(),
            None => continue,
        };

        // Collect all matching rules with their specificity
        let mut matched: Vec<(&parser::CSSRule, (u32, u32, u32))> = Vec::new();

        for rule in &stylesheet.rules {
            for selector in &rule.selectors {
                if selector.matches_element(
                    &tag,
                    |c| node_has_class(&node, c),
                    |i| node_get_id(&node) == Some(i),
                ) {
                    matched.push((rule, selector.specificity()));
                }
            }
        }

        // CSS Cascade order (author origin only):
        // 1. Normal declarations (applied first — can be overridden)
        // 2. !important declarations (applied last — override normal)
        // Within each group: sort by specificity ascending (lower first, higher overwrites).
        // Stable sort preserves source order within same specificity (later rule wins).

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

        // Pass 1: Normal declarations (ascending specificity — higher wins by overwriting)
        normal_decls.sort_by(|a, b| a.1.cmp(&b.1));
        for (decl, _) in normal_decls {
            computed = computed.from_declaration(decl);
        }

        // Pass 2: !important declarations (override normal, ascending specificity)
        important_decls.sort_by(|a, b| a.1.cmp(&b.1));
        for (decl, _) in important_decls {
            computed = computed.from_declaration(decl);
        }

        result.insert(node_id, computed);
    }

    // Phase 2: Inheritance — BFS from root to propagate inheritable properties
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

    // Topological sort: process nodes in document order (BFS-ish)
    // Since arena assigns IDs in document order, processing in ascending ID order
    // ensures parents are processed before children.
    for &node_id in &element_ids {
        if let Some(&parent_id) = parent_map.get(&node_id) {
            if let Some(parent_styles) = result.get(&parent_id) {
                let child_styles = result.get(&node_id).cloned().unwrap_or_default();
                let inherited = inherit_properties(parent_styles, child_styles);
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

// ------ Computed Values ------

/// Fully resolved CSS property values for a single element.
#[derive(Debug, Clone)]
pub struct ComputedValues {
    pub display: DisplayType,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Margin: [top, right, bottom, left]
    pub margin: [f32; 4],
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
    /// Flex basis (None = auto)
    pub flex_basis: Option<f32>,
}

impl Default for ComputedValues {
    /// CSS initial values per the CSS specification.
    fn default() -> Self {
        Self {
            display: DisplayType::Inline,
            width: None,
            height: None,
            margin: [0.0; 4],
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
            row_gap: 0.0,
            column_gap: 0.0,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: None,
        }
    }
}

impl ComputedValues {
    /// Parse a single [`Declaration`] and apply it to a clone of `self`.
    ///
    /// Returns a new `ComputedValues` with the declaration applied on top
    /// of the current values (or defaults if called on `Default::default()`).
    pub fn from_declaration(mut self, decl: &Declaration) -> Self {
        let prop = decl.property.to_lowercase();
        let val = decl.value.trim();

        match prop.as_str() {
            "display" => {
                self.display = match val {
                    "block" => DisplayType::Block,
                    "inline" => DisplayType::Inline,
                    "inline-block" => DisplayType::InlineBlock,
                    "flex" => DisplayType::Flex,
                    "inline-flex" => DisplayType::InlineFlex,
                    "none" => DisplayType::None,
                    _ => self.display,
                };
            }
            "width" => {
                self.width = parse_length(val);
            }
            "height" => {
                self.height = parse_length(val);
            }
            "margin" => {
                self.margin = parse_box_four(val, self.margin);
            }
            "margin-top" => {
                if let Some(v) = parse_length(val) {
                    self.margin[0] = v;
                }
            }
            "margin-right" => {
                if let Some(v) = parse_length(val) {
                    self.margin[1] = v;
                }
            }
            "margin-bottom" => {
                if let Some(v) = parse_length(val) {
                    self.margin[2] = v;
                }
            }
            "margin-left" => {
                if let Some(v) = parse_length(val) {
                    self.margin[3] = v;
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
                parse_flex_shorthand(&mut self, val);
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
            _ => {}
        }

        self
    }
}

/// Parse a CSS length value (pixels) from a string like `"10px"` or `"10"`.
/// Returns `None` for `"auto"`, `"inherit"`, or unparseable values.
fn parse_length(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("inherit") {
        return None;
    }
    // Strip common unit suffixes and parse as pixels
    let num = s.trim_end_matches(|c: char| c.is_alphabetic());
    if num.is_empty() {
        return None;
    }
    num.parse::<f32>().ok()
}

/// Parse a box model shorthand (margin/padding) into four values.
///
/// Supports 1, 2, 3, or 4 space-separated values:
/// - 1 value: all sides
/// - 2 values: vertical, horizontal
/// - 3 values: top, horizontal, bottom
/// - 4 values: top, right, bottom, left
fn parse_box_four(s: &str, fallback: [f32; 4]) -> [f32; 4] {
    let parts: Vec<f32> = s
        .split_whitespace()
        .filter_map(|p| parse_length(p))
        .collect();

    match parts.len() {
        1 => [parts[0]; 4],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        4 => [parts[0], parts[1], parts[2], parts[3]],
        _ => fallback,
    }
}

/// Parse flex-basis value: "auto" -> None, numeric length -> Some(px).
fn parse_flex_basis(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("content") {
        return None;
    }
    // Percentages treated as auto for now (simplification)
    if s.ends_with('%') {
        return None;
    }
    parse_length(s)
}

/// Parse the flex shorthand property: "flex: <grow> <shrink>? <basis>?"
fn parse_flex_shorthand(self_vals: &mut ComputedValues, val: &str) {
    let parts: Vec<&str> = val.trim().split_whitespace().collect();

    if parts.is_empty() {
        return;
    }

    // Special case: "flex: none" -> grow=0, shrink=0, basis=0px
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
        self_vals.flex_grow = 0.0;
        self_vals.flex_shrink = 0.0;
        self_vals.flex_basis = Some(0.0);
        return;
    }

    // "flex: auto" -> grow=1, shrink=1, basis=auto
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("auto") {
        self_vals.flex_grow = 1.0;
        self_vals.flex_shrink = 1.0;
        self_vals.flex_basis = None;
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
        // Second part: try as number (shrink), else try as length (basis)
        if let Ok(shrink) = parts[1].parse::<f32>() {
            self_vals.flex_shrink = shrink;
        } else {
            self_vals.flex_basis = parse_flex_basis(parts[1]);
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
        self_vals.flex_basis = parse_flex_basis(parts[2]);
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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        assert_eq!(computed.flex_basis, Some(100.0));
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
        assert_eq!(computed.flex_basis, Some(0.0));
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
        assert_eq!(computed.flex_basis, None);
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
        assert_eq!(computed.flex_basis, None); // default
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
        assert_eq!(computed.flex_basis, None);
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
}
