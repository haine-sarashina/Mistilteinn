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
    // `line-height` inherits
    if child.line_height == 1.2 && parent.line_height != 1.2 {
        child.line_height = parent.line_height;
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

        // Pass 3: Inline `style` attribute — highest specificity per CSS spec.
        // Applied after stylesheet cascade but before inheritance propagation.
        if let Some(style_attr) = node.get_attr("style") {
            let inline_decls = parse_declarations(style_attr);
            for decl in &inline_decls {
                computed = computed.from_declaration(decl);
            }
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

// ------ Positioning ------

/// The computed `position` CSS property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionType {
    Static,
    Relative,
    Absolute,
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
            flex_basis: FlexBasis::Auto,
            box_sizing: BoxSizing::ContentBox,
            explicit_width: None,
            explicit_height: None,
            min_width: None,
            max_width: None,
            line_height: 1.2, // CSS default for "normal"
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            position: PositionType::Static,
            offset_top: None,
            offset_right: None,
            offset_bottom: None,
            offset_left: None,
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
                self.explicit_width = self.width;
            }
            "height" => {
                self.height = parse_length(val);
                self.explicit_height = self.height;
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
            _ => {}
        }

        self
    }
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

/// Parse flex-basis value: "auto" -> FlexBasis::Auto, numeric length -> FlexBasis::Pixels(px),
/// percentage value (e.g. "50%") -> FlexBasis::Percentage(frac).
fn parse_flex_basis(s: &str) -> FlexBasis {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("content") {
        return FlexBasis::Auto;
    }
    if let Some(pct_val) = s.strip_suffix('%') {
        if let Ok(n) = pct_val.parse::<f32>() {
            return FlexBasis::Percentage(n / 100.0);
        }
    }
    if let Some(px_val) = s.strip_suffix("px") {
        if let Ok(n) = px_val.parse::<f32>() {
            return FlexBasis::Pixels(n);
        }
    }
    FlexBasis::Auto
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
        ("table", "display: table"),
        ("img", "display: inline"),
        ("a", "display: inline"),
        ("div", "display: block"),
        ("span", "display: inline"),
        ("form", "display: block"),
        ("input", "display: inline"),
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

    parser::Stylesheet { rules }
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
    parser::Stylesheet { rules }
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
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        assert_eq!(div_styles.color, Some([0, 0, 255, 255])); // blue
    }

    #[test]
    fn inline_style_overrides_stylesheet() {
        let arena = crate::html::parse_html(r#"<div id="mydiv" style="color: green;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { color: red; } #mydiv { color: blue; }");
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
        assert_eq!(div_styles.color, Some([0, 128, 0, 255])); // green from inline
    }

    #[test]
    fn inline_style_multiple_properties() {
        let arena = crate::html::parse_html(
            r#"<div style="color: red; font-size: 20px; background-color: yellow;">Test</div>"#,
        );
        let stylesheet = parser::parse_stylesheet("");
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
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // red
        assert_eq!(div_styles.font_size, 20.0);
        assert_eq!(div_styles.background_color, Some([255, 255, 0, 255])); // yellow
    }

    #[test]
    fn inline_style_combines_with_stylesheet() {
        let arena = crate::html::parse_html(r#"<div style="color: red;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { font-size: 18px; }");
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
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // from inline
        assert_eq!(div_styles.font_size, 18.0); // from stylesheet
    }

    #[test]
    fn inline_style_overrides_important() {
        let arena = crate::html::parse_html(r#"<div style="color: red;">Test</div>"#);
        let stylesheet = parser::parse_stylesheet("div { color: blue !important; }");
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
        assert_eq!(div_styles.color, Some([255, 0, 0, 255])); // inline wins
    }

    #[test]
    fn inline_style_no_style_attribute() {
        let arena = crate::html::parse_html("<div>No inline style</div>");
        let stylesheet = parser::parse_stylesheet("div { color: green; }");
        let styles = compute_styles_for_tree(&arena, &stylesheet);

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
        let styles = compute_styles_for_tree(&arena, &merged);

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
        let styles = compute_styles_for_tree(&arena, &merged);

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
        let styles = compute_styles_for_tree(&arena, &merged);

        let nodes = arena.nodes.borrow();
        let p_id = nodes.iter().position(|n| {
            n.is_element() && n.tag_name().map(|t| t.to_string() == "p").unwrap_or(false)
        });
        if let Some(id) = p_id {
            let p_styles = styles.get(&(id as u32)).expect("p has styles");
            assert_eq!(p_styles.display, DisplayType::Block, "p should be block");
            assert_eq!(p_styles.margin[0], 1.0, "p top margin from UA");
            assert_eq!(p_styles.margin[2], 1.0, "p bottom margin from UA");
        } else {
            assert!(false, "Expected to find a <p> node");
        }
    }

    #[test]
    fn author_style_overrides_ua() {
        let arena = crate::html::parse_html("<html><body><div>test</div></body></html>");
        let ua = user_agent_stylesheet();
        let author = parser::parse_stylesheet("div { display: inline; }");
        let merged = merge_stylesheets_with_author(&ua, &author);
        let styles = compute_styles_for_tree(&arena, &merged);

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
}
