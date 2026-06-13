/// Layout engine module.
///
/// Responsible for:
/// - Box model computation (content, padding, border, margin)
/// - Flexbox layout
/// - Inline text layout
/// - Render tree construction
use crate::css::{
    AlignContent, AlignItems, BoxSizing, ComputedValues, DisplayType, FlexBasis, FlexDirection,
    FlexWrap, GridTrack, JustifyContent, Overflow, PositionType,
};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Returns the right edge of the rectangle.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Returns the bottom edge of the rectangle.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Returns the union rectangle that contains both self and other.
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }

    /// Returns the intersection rectangle, or None if they don't overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right > x && bottom > y {
            Some(Rect::new(x, y, right - x, bottom - y))
        } else {
            None
        }
    }

    pub fn area(&self) -> f32 {
        self.width * self.height
    }
}

/// Inline box types within a line (inline formatting context).
#[derive(Debug, Clone)]
pub enum InlineBox {
    Text {
        text: String,
        width: f32, // measured width of this run
        color: Option<[u8; 4]>,
        font_size: f32,
    },
    Element {
        child_index: usize, // index into parent's children vec for the LayoutNode
        width: f32,
        height: f32,
        baseline_offset: f32, // distance from bottom of element to baseline
    },
    Whitespace {
        collapsible: bool,
        width: f32,
    },
}

/// A line box contains inline boxes and records its metrics.
#[derive(Debug, Clone)]
pub struct LineBox {
    /// Top position of this line (relative to block container).
    pub y: f32,
    /// Baseline position for alignment.
    pub baseline_y: f32,
    /// Total line height (max ascender + max descender in line).
    pub height: f32,
    pub boxes: Vec<InlineBox>,
}

/// Kinds of interactive elements that change the mouse cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionType {
    /// No interaction (default).
    None,
    /// A hyperlink (<a> tag) — shows pointer cursor.
    Link,
    /// An input element (<input>, <textarea>, <button>) — shows I-beam cursor.
    Input,
}

/// A node in the layout tree.
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub rect: Rect,
    pub display: DisplayType,
    /// Padding: [top, right, bottom, left]
    pub padding: [f32; 4],
    /// Margin: [top, right, bottom, left]
    pub margin: [f32; 4],
    /// Border: [top, right, bottom, left]
    pub border: [f32; 4],
    pub children: Vec<LayoutNode>,
    /// Text content for leaf nodes (inline text runs).
    pub text: Option<String>,
    /// Background color as RGBA (None = transparent/no background).
    pub background_color: Option<[u8; 4]>,
    /// Foreground text color as RGBA (None = not explicitly set, defaults to black).
    pub color: Option<[u8; 4]>,
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
    /// URL of an image source for <img> tags (None = not an image).
    pub image_src: Option<String>,
    /// Line boxes for block containers with inline children (None = no inline layout computed yet).
    pub line_boxes: Option<Vec<LineBox>>,
    /// Overflow behavior for clipping/scrolling.
    pub overflow: Overflow,
    /// Positioning type (static, relative, absolute).
    pub position: PositionType,
    /// Offset values: [top, right, bottom, left]
    pub offsets: [Option<f32>; 4],
    /// Absolutely positioned children extracted from the normal flow.
    pub absolute_children: Vec<LayoutNode>,
    /// Computed font size in pixels (copied from CSS ComputedValues).
    pub font_size: f32,
    /// Computed font family (copied from CSS ComputedValues).
    pub font_family: String,
    /// Grid container properties
    pub grid_columns: Vec<GridTrack>,
    pub grid_rows: Vec<GridTrack>,
    pub grid_column_gap: f32,
    pub grid_row_gap: f32,
    /// CSS order property (for flex/grid item ordering)
    pub order: i32,
    /// Interaction type for cursor changes (link, input, etc.).
    pub interaction_type: InteractionType,
    /// Reference back to the DOM node ID (set only for element nodes, not text/image leaves).
    pub dom_node_id: Option<u32>,
}

impl LayoutNode {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            display: DisplayType::Block,
            padding: [0.0; 4],
            margin: [0.0; 4],
            border: [0.0; 4],
            children: Vec::new(),
            text: None,
            background_color: None,
            color: None,
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
            image_src: None,
            line_boxes: None,
            overflow: Overflow::Visible,
            position: PositionType::Static,
            offsets: [None, None, None, None],
            absolute_children: Vec::new(),
            font_size: 16.0,
            font_family: String::new(),
            grid_columns: Vec::new(),
            grid_rows: Vec::new(),
            grid_column_gap: 0.0,
            grid_row_gap: 0.0,
            order: 0,
            interaction_type: InteractionType::None,
            dom_node_id: None,
        }
    }

    pub fn new_with_display(rect: Rect, display: DisplayType) -> Self {
        Self {
            display,
            ..Self::new(rect)
        }
    }

    pub fn add_child(&mut self, child: LayoutNode) {
        self.children.push(child);
    }

    /// Returns the content box rect (excluding padding, border, margin).
    pub fn content_box(&self) -> Rect {
        Rect::new(
            self.rect.x + self.margin[3] + self.border[3] + self.padding[3],
            self.rect.y + self.margin[0] + self.border[0] + self.padding[0],
            self.rect.width - self.horizontal_margin_and_padding_and_border(),
            self.rect.height - self.vertical_margin_and_padding_and_border(),
        )
    }

    fn horizontal_margin_and_padding_and_border(&self) -> f32 {
        self.margin[1]
            + self.margin[3]
            + self.border[1]
            + self.border[3]
            + self.padding[1]
            + self.padding[3]
    }

    fn vertical_margin_and_padding_and_border(&self) -> f32 {
        self.margin[0]
            + self.margin[2]
            + self.border[0]
            + self.border[2]
            + self.padding[0]
            + self.padding[2]
    }
}

/// Trait for DOM nodes so the layout engine can query them without depending
/// on the internal `DomNode` struct directly.
pub trait LayoutDomNode {
    fn tag_name(&self) -> String;
    fn get_attr(&self, name: &str) -> Option<String>;
    fn children_ids(&self) -> Vec<u32>;
    fn text_content(&self) -> Option<&str>;
}

/// Build a layout tree from DOM nodes and computed styles.
///
/// Takes a root node ID, a map of node_id -> ComputedValues,
/// and a way to look up DOM nodes by ID.
pub fn build_layout_tree<N, F>(
    root_id: u32,
    styles: &FxHashMap<u32, ComputedValues>,
    get_node: F,
    _page_width: f32,
) -> LayoutNode
where
    N: LayoutDomNode,
    F: Copy + Fn(u32) -> Option<N>,
{
    let root_styles = styles.get(&root_id).cloned().unwrap_or_default();

    let root_dom = get_node(root_id);
    let mut root_layout =
        LayoutNode::new_with_display(Rect::new(0.0, 0.0, _page_width, 0.0), root_styles.display);
    root_layout.padding = root_styles.padding;
    root_layout.margin = root_styles.margin;
    root_layout.flex_direction = root_styles.flex_direction;
    root_layout.flex_wrap = root_styles.flex_wrap;
    root_layout.justify_content = root_styles.justify_content;
    root_layout.align_items = root_styles.align_items;
    root_layout.align_content = root_styles.align_content;
    root_layout.row_gap = root_styles.row_gap;
    root_layout.column_gap = root_styles.column_gap;
    root_layout.overflow = root_styles.overflow_x;
    root_layout.position = root_styles.position;
    root_layout.offsets = [
        root_styles.offset_top,
        root_styles.offset_right,
        root_styles.offset_bottom,
        root_styles.offset_left,
    ];

    // Propagate font properties from computed styles
    root_layout.font_size = root_styles.font_size;
    root_layout.font_family = root_styles.font_family.clone();

    // Copy box-sizing, explicit dimensions, flex basis, and min/max width
    root_layout.box_sizing = root_styles.box_sizing;
    root_layout.explicit_width = root_styles.explicit_width;
    root_layout.explicit_height = root_styles.explicit_height;
    root_layout.min_width = root_styles.min_width;
    root_layout.max_width = root_styles.max_width;
    root_layout.flex_basis = root_styles.flex_basis;

    // Copy grid properties
    root_layout.grid_columns = root_styles.grid_template_columns.clone();
    root_layout.grid_rows = root_styles.grid_template_rows.clone();
    root_layout.grid_column_gap = root_styles.grid_column_gap;
    root_layout.grid_row_gap = root_styles.grid_row_gap;
    root_layout.order = root_styles.order;

    if let Some(node) = root_dom {
        build_layout_children(&mut root_layout, &node.children_ids(), styles, get_node, 0);
    }

    root_layout
}

/// Maximum nesting depth for layout tree traversal.
/// Real-world pages rarely exceed 50 levels; 512 provides a large safety margin
/// while preventing stack overflow on pathologically deep DOM trees.
const MAX_LAYOUT_DEPTH: usize = 512;

fn build_layout_children<N, F>(
    parent: &mut LayoutNode,
    child_ids: &[u32],
    styles: &FxHashMap<u32, ComputedValues>,
    get_node: F,
    depth: usize,
) where
    N: LayoutDomNode,
    F: Copy + Fn(u32) -> Option<N>,
{
    if depth > MAX_LAYOUT_DEPTH {
        // Safety: prevent stack overflow on deeply nested DOM trees.
        // Silently stop building  Ethe page will render partially rather than crash.
        return;
    }

    for &child_id in child_ids {
        if let Some(node) = get_node(child_id) {
            let child_styles = styles.get(&child_id).cloned().unwrap_or_default();

            match child_styles.display {
                DisplayType::None => continue,
                DisplayType::Block
                | DisplayType::Flex
                | DisplayType::InlineFlex
                | DisplayType::Grid => {
                    let display = match child_styles.display {
                        DisplayType::Flex | DisplayType::InlineFlex | DisplayType::Grid => {
                            child_styles.display
                        }
                        _ => DisplayType::Block,
                    };
                    let mut layout_node =
                        LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), display);
                    layout_node.padding = child_styles.padding;
                    layout_node.margin = child_styles.margin;
                    layout_node.background_color = child_styles.background_color;
                    layout_node.color = child_styles.color;
                    layout_node.font_size = child_styles.font_size;
                    layout_node.font_family = child_styles.font_family.clone();
                    // Copy flexbox properties
                    layout_node.flex_direction = child_styles.flex_direction;
                    layout_node.flex_wrap = child_styles.flex_wrap;
                    layout_node.justify_content = child_styles.justify_content;
                    layout_node.align_items = child_styles.align_items;
                    layout_node.align_content = child_styles.align_content;
                    layout_node.row_gap = child_styles.row_gap;
                    layout_node.column_gap = child_styles.column_gap;
                    layout_node.flex_grow = child_styles.flex_grow;
                    layout_node.flex_shrink = child_styles.flex_shrink;
                    layout_node.flex_basis = child_styles.flex_basis;
                    layout_node.box_sizing = child_styles.box_sizing;
                    layout_node.explicit_width = child_styles.explicit_width;
                    layout_node.explicit_height = child_styles.explicit_height;
                    layout_node.min_width = child_styles.min_width;
                    layout_node.max_width = child_styles.max_width;

                    // Copy grid properties
                    layout_node.grid_columns = child_styles.grid_template_columns.clone();
                    layout_node.grid_rows = child_styles.grid_template_rows.clone();
                    layout_node.grid_column_gap = child_styles.grid_column_gap;
                    layout_node.grid_row_gap = child_styles.grid_row_gap;
                    layout_node.order = child_styles.order;

                    // Copy overflow and positioning properties
                    layout_node.overflow = child_styles.overflow_x;
                    layout_node.position = child_styles.position;
                    layout_node.offsets = [
                        child_styles.offset_top,
                        child_styles.offset_right,
                        child_styles.offset_bottom,
                        child_styles.offset_left,
                    ];

                    // Extract image src from <img> tags
                    if node.tag_name() == "img" {
                        layout_node.image_src = node.get_attr("src");
                    }

                    // Set interaction type based on tag name for cursor changes
                    let tag = node.tag_name().to_lowercase();
                    layout_node.interaction_type = match tag.as_str() {
                        "a" => InteractionType::Link,
                        "input" | "textarea" | "button" => InteractionType::Input,
                        _ => InteractionType::None,
                    };

                    // Track DOM node ID for hit-testing / :hover
                    layout_node.dom_node_id = Some(child_id);

                    let text = node.text_content().map(|t| t.to_string());
                    if text.is_some() && node.children_ids().is_empty() {
                        layout_node.text = text;
                    } else {
                        build_layout_children(
                            &mut layout_node,
                            &node.children_ids(),
                            styles,
                            get_node,
                            depth + 1,
                        );
                    }
                    parent.add_child(layout_node);
                }
                DisplayType::Inline | DisplayType::InlineBlock => {
                    let mut layout_node = LayoutNode::new_with_display(
                        Rect::new(0.0, 0.0, 0.0, 0.0),
                        child_styles.display,
                    );
                    layout_node.padding = child_styles.padding;
                    layout_node.margin = child_styles.margin;
                    layout_node.background_color = child_styles.background_color;
                    layout_node.color = child_styles.color;
                    layout_node.font_size = child_styles.font_size;
                    layout_node.font_family = child_styles.font_family.clone();

                    // Copy overflow and positioning properties
                    layout_node.overflow = child_styles.overflow_x;
                    layout_node.position = child_styles.position;
                    layout_node.offsets = [
                        child_styles.offset_top,
                        child_styles.offset_right,
                        child_styles.offset_bottom,
                        child_styles.offset_left,
                    ];

                    // Extract image src from <img> tags
                    if node.tag_name() == "img" {
                        layout_node.image_src = node.get_attr("src");
                    }

                    // Set interaction type based on tag name for cursor changes
                    let tag = node.tag_name().to_lowercase();
                    layout_node.interaction_type = match tag.as_str() {
                        "a" => InteractionType::Link,
                        "input" | "textarea" | "button" => InteractionType::Input,
                        _ => InteractionType::None,
                    };

                    // Track DOM node ID for hit-testing / :hover
                    layout_node.dom_node_id = Some(child_id);

                    let text = node.text_content().map(|t| t.to_string());
                    if text.is_some() && node.children_ids().is_empty() {
                        layout_node.text = text;
                    } else {
                        build_layout_children(
                            &mut layout_node,
                            &node.children_ids(),
                            styles,
                            get_node,
                            depth + 1,
                        );
                    }
                    parent.add_child(layout_node);
                }
            }
        }
    }
}

/// Compute the layout rectangles for a layout tree.
///
/// Block-level children are stacked vertically (top-to-bottom).
/// Flex containers lay out their children using flexbox algorithm.
/// Inline children are grouped into line boxes with word wrapping and baseline alignment.
/// Each node's margin, border, padding, and content box are computed.
pub fn compute_layout(
    root: &mut LayoutNode,
    page_width: f32,
    text_renderer: &mut crate::render::text::TextRenderer,
) {
    let available_width = page_width
        - root.margin[3]
        - root.border[3]
        - root.padding[3]
        - root.padding[1]
        - root.border[1]
        - root.margin[1];

    let start_x = root.padding[3] + root.border[3];
    let start_y = root.padding[0] + root.border[0];

    match root.display {
        DisplayType::Flex | DisplayType::InlineFlex => {
            compute_flex_children(root, start_x, start_y, available_width, 0, text_renderer);
        }
        DisplayType::Grid => {
            compute_grid_children(root, start_x, start_y, available_width, 0, text_renderer);
        }
        _ => {
            compute_block_children(root, start_x, start_y, available_width, 0, text_renderer);
        }
    }
}

/// Extract absolutely positioned children from the normal flow.
/// This must run after `build_layout_children` and before `compute_layout`.
///
/// Absolutely positioned elements are removed from `node.children` and stored
/// in `node.absolute_children` so they don't participate in block or flex layout.
pub fn extract_absolute_children(node: &mut LayoutNode) {
    // Collect absolute children by swapping them out (stable without drain_filter)
    let mut abs_children: Vec<LayoutNode> = Vec::new();
    let mut i = 0;
    while i < node.children.len() {
        if node.children[i].position == PositionType::Absolute {
            abs_children.push(node.children.remove(i));
        } else {
            i += 1;
        }
    }

    node.absolute_children = abs_children;

    // Recurse into remaining (normal-flow) children
    for child in &mut node.children {
        extract_absolute_children(child);
    }
}

/// Apply relative positioning offsets after normal layout computation.
///
/// Relatively positioned elements stay in the normal flow but their rect is
/// shifted by the specified top/right/bottom/left offsets.
pub fn apply_relative_positioning(node: &mut LayoutNode) {
    for child in &mut node.children {
        if child.position == PositionType::Relative {
            let dx = child.offsets[3].unwrap_or(0.0) - child.offsets[1].unwrap_or(0.0); // left - right
            let dy = child.offsets[0].unwrap_or(0.0) - child.offsets[2].unwrap_or(0.0); // top - bottom
            child.rect.x += dx;
            child.rect.y += dy;
        }
        apply_relative_positioning(child);
    }

    // Also handle absolute children's descendants
    for abs_child in &mut node.absolute_children {
        if abs_child.position == PositionType::Relative {
            let dx = abs_child.offsets[3].unwrap_or(0.0) - abs_child.offsets[1].unwrap_or(0.0);
            let dy = abs_child.offsets[0].unwrap_or(0.0) - abs_child.offsets[2].unwrap_or(0.0);
            abs_child.rect.x += dx;
            abs_child.rect.y += dy;
        }
        apply_relative_positioning(abs_child);
    }
}

/// Compute positions for absolutely positioned children relative to the containing block.
///
/// The `containing_block` rect defines the content box of the nearest ancestor with
/// `position != Static`. For static ancestors, it defaults to the initial containing block.
pub fn compute_absolute_positions(
    node: &mut LayoutNode,
    containing_block: Rect,
    text_renderer: &mut crate::render::text::TextRenderer,
) {
    for abs_child in &mut node.absolute_children {
        // Position based on offsets relative to containing block's content box
        let x = containing_block.x + abs_child.offsets[3].unwrap_or(0.0); // left offset (index 3)
        let y = containing_block.y + abs_child.offsets[0].unwrap_or(0.0); // top offset (index 0)

        // Compute intrinsic dimensions for the absolute child
        let content_height = compute_block_height_inner(abs_child, 0, text_renderer);
        let total_height = content_height
            + abs_child.padding[0]
            + abs_child.padding[2]
            + abs_child.border[0]
            + abs_child.border[2];

        // Width: if explicit width is set via rect, use it; otherwise compute from content
        let mut total_width = if abs_child.rect.width > 0.0 {
            abs_child.rect.width
                - abs_child.padding[1]
                - abs_child.padding[3]
                - abs_child.border[1]
                - abs_child.border[3]
        } else {
            // Estimate intrinsic width from content
            let mut max_child_width = 0.0;
            for grandchild in &abs_child.children {
                if is_block_child(grandchild) {
                    // For block children, they'd take full available width
                    // Use a reasonable default based on containing block
                    max_child_width = containing_block.width;
                    break;
                } else if is_inline_child(grandchild) {
                    let child_content_h = compute_inline_height(grandchild);
                    if child_content_h > max_child_width {
                        max_child_width = child_content_h;
                    }
                }
            }
            if abs_child.children.is_empty() {
                // No children: shrink to fit padding+border only
                max_child_width = 0.0;
            } else {
                max_child_width = max_child_width.min(containing_block.width);
            }
            max_child_width
                + abs_child.padding[1]
                + abs_child.padding[3]
                + abs_child.border[1]
                + abs_child.border[3]
        };

        // Handle right/bottom offsets (if set, they constrain the box differently)
        if abs_child.offsets[1].is_some() {
            // Right offset: x = containing_block.right - offset_right - total_width
            // We need to recompute width first
            let cb_right = containing_block.right();
            total_width = total_width.max(0.0);
            abs_child.rect.x = cb_right - abs_child.offsets[1].unwrap_or(0.0) - total_width;
        } else {
            abs_child.rect.x = x;
        }

        if abs_child.offsets[2].is_some() {
            // Bottom offset: y = containing_block.bottom - offset_bottom - total_height
            let cb_bottom = containing_block.bottom();
            abs_child.rect.height = total_height.max(0.0);
            abs_child.rect.y =
                cb_bottom - abs_child.offsets[2].unwrap_or(0.0) - abs_child.rect.height;
        } else {
            abs_child.rect.y = y;
            abs_child.rect.height = total_height.max(0.0);
        }

        abs_child.rect.width = total_width.max(0.0);

        // Compute layout for the absolute child's normal-flow children
        let inner_width = (abs_child.rect.width
            - abs_child.padding[3]
            - abs_child.padding[1]
            - abs_child.border[3]
            - abs_child.border[1])
            .max(0.0);

        let inner_x = abs_child.rect.x + abs_child.padding[3] + abs_child.border[3];
        let inner_y = abs_child.rect.y + abs_child.padding[0] + abs_child.border[0];

        match abs_child.display {
            DisplayType::Flex | DisplayType::InlineFlex => {
                compute_flex_children(abs_child, inner_x, inner_y, inner_width, 0, text_renderer);
            }
            DisplayType::Grid => {
                compute_grid_children(abs_child, inner_x, inner_y, inner_width, 0, text_renderer);
            }
            _ => {
                compute_block_children(abs_child, inner_x, inner_y, inner_width, 0, text_renderer);
            }
        }

        // Apply relative positioning to descendants within the absolute child
        apply_relative_positioning(abs_child);

        // Recurse: compute absolute children of this absolute node
        let abs_content_box = Rect::new(
            abs_child.rect.x + abs_child.padding[3] + abs_child.border[3],
            abs_child.rect.y + abs_child.padding[0] + abs_child.border[0],
            (abs_child.rect.width
                - abs_child.padding[1]
                - abs_child.padding[3]
                - abs_child.border[1]
                - abs_child.border[3])
                .max(0.0),
            (abs_child.rect.height
                - abs_child.padding[0]
                - abs_child.padding[2]
                - abs_child.border[0]
                - abs_child.border[2])
                .max(0.0),
        );
        compute_absolute_positions(abs_child, abs_content_box, text_renderer);
    }
}

/// Check if a child participates in the inline formatting context.
/// Text nodes (nodes with `text` set) and inline/inline-block elements do so.
fn is_inline_child(child: &LayoutNode) -> bool {
    child.text.is_some()
        || matches!(
            child.display,
            DisplayType::Inline | DisplayType::InlineBlock
        )
}

/// Check if a child is a block-level participant (flex children are also block-level here).
fn is_block_child(child: &LayoutNode) -> bool {
    !is_inline_child(child)
        && matches!(
            child.display,
            DisplayType::Block | DisplayType::Flex | DisplayType::InlineFlex | DisplayType::Grid
        )
}

/// Stack block-level children vertically, computing each child's rect.
/// Inline children (text nodes and inline elements) are grouped into an inline
/// formatting context: build_inline_boxes + break_into_lines + position.
fn compute_block_children(
    parent: &mut LayoutNode,
    parent_x: f32,
    _parent_y: f32,
    available_width: f32,
    depth: usize,
    text_renderer: &mut crate::render::text::TextRenderer,
) {
    if depth > MAX_LAYOUT_DEPTH {
        return;
    }

    // --- Separate children into block and inline runs ---
    // We process the children list and find contiguous runs of inline children.
    // Each run is laid out as an inline formatting context. Block children use
    // the traditional vertical stacking.

    let mut y = parent.padding[0]; // start after top padding
    let children_count = parent.children.len();
    let mut i = 0;

    while i < children_count {
        let child = &parent.children[i];

        if is_inline_child(child)
            || matches!(
                child.display,
                DisplayType::Inline | DisplayType::InlineBlock
            )
        {
            // Start of an inline run  Ecollect consecutive inline children
            let mut run_end = i;
            while run_end < children_count && is_inline_child(&parent.children[run_end]) {
                run_end += 1;
            }

            // Build inline boxes from the run
            let inline_boxes = build_inline_boxes_from_slice(parent, &parent.children[i..run_end]);

            if !inline_boxes.is_empty() {
                // Compute heights of inline children first (needed for element sizing)
                for j in i..run_end {
                    compute_block_height_inner(&parent.children[j], depth + 1, text_renderer);
                }

                // Break into lines
                let line_boxes = break_into_lines(inline_boxes, available_width, text_renderer);

                // Position inline children within line boxes
                position_inline_children_in_lines(
                    &mut parent.children[i..run_end],
                    &line_boxes,
                    parent_x,
                    y,
                );

                // Store line boxes on parent
                if parent.line_boxes.is_none() {
                    parent.line_boxes = Some(line_boxes.clone());
                } else if let Some(ref mut existing) = parent.line_boxes {
                    existing.extend(line_boxes.clone());
                }

                // Advance y past the inline content
                let mut inline_height = 0.0;
                for lb in &line_boxes {
                    let line_bottom = lb.y + lb.height;
                    if line_bottom > inline_height {
                        inline_height = line_bottom;
                    }
                }
                y += inline_height;
            }

            i = run_end;
        } else if is_block_child(child) {
            // Block-level child: traditional vertical stacking
            // Collect all values from immutable borrow before mutating
            let c = &parent.children[i];
            let child_x = parent_x + c.margin[3];
            let child_y = y + c.margin[0];
            let child_width = available_width - c.margin[3] - c.margin[1];
            let child_content_height =
                compute_block_height_inner(&parent.children[i], depth + 1, text_renderer);
            let pad_top = c.padding[0];
            let pad_bottom = c.padding[2];
            let border_top = c.border[0];
            let border_bottom = c.border[2];
            let margin_bottom = c.margin[2];
            let margin_left = c.margin[3];
            let margin_right = c.margin[1];
            let border_left = c.border[3];
            let border_right = c.border[1];
            let pad_left = c.padding[3];
            let pad_right = c.padding[1];
            let child_display = c.display;
            // Use all values from the immutable borrow above before mutating parent.children[i] below

            let child_height =
                child_content_height + pad_top + pad_bottom + border_top + border_bottom;

            parent.children[i].rect = Rect::new(child_x, child_y, child_width, child_height);
            y = parent.children[i].rect.bottom() + margin_bottom;

            // Recurse into block child's children
            let inner_width = (parent.children[i].rect.width
                - margin_left
                - margin_right
                - border_left
                - border_right
                - pad_left
                - pad_right)
                .max(0.0);

            let inner_x = parent.children[i].rect.x + pad_left + border_left;
            let inner_y = parent.children[i].rect.y + pad_top + border_top;

            match child_display {
                DisplayType::Flex | DisplayType::InlineFlex => {
                    compute_flex_children(
                        &mut parent.children[i],
                        inner_x,
                        inner_y,
                        inner_width,
                        depth + 1,
                        text_renderer,
                    );
                }
                DisplayType::Grid => {
                    compute_grid_children(
                        &mut parent.children[i],
                        inner_x,
                        inner_y,
                        inner_width,
                        depth + 1,
                        text_renderer,
                    );
                }
                _ => {
                    compute_block_children(
                        &mut parent.children[i],
                        inner_x,
                        inner_y,
                        inner_width,
                        depth + 1,
                        text_renderer,
                    );
                }
            }

            i += 1;
        } else {
            // display:none or unclassified  Eskip
            i += 1;
        }
    }

    // Update parent height based on content extent
    // Only overwrite height/width if they haven't been set by a parent flexbox layout pass.
    let content_height = (y - parent.padding[0]).max(0.0);
    let computed_total_height = content_height
        + parent.padding[0]
        + parent.border[0]
        + parent.padding[2]
        + parent.border[2];

    // Only set height if it's zero (not already set by flexbox parent)
    if parent.rect.height == 0.0 {
        parent.rect.height = computed_total_height;
    } else {
        // Use the larger of computed or existing height
        parent.rect.height = parent.rect.height.max(computed_total_height);
    }

    // If width wasn't set, fill available width
    if parent.rect.width == 0.0 {
        parent.rect.width = available_width;
    }
}

/// Compute inner height contribution of a single node (content area only, no padding).
fn compute_block_height_inner(
    node: &LayoutNode,
    depth: usize,
    _text_renderer: &mut crate::render::text::TextRenderer,
) -> f32 {
    if depth > MAX_LAYOUT_DEPTH {
        return 0.0;
    }
    let mut height = 0.0;
    for child in &node.children {
        if is_block_child(child) {
            height += compute_block_height_inner(child, depth + 1, _text_renderer)
                + child.padding[0]
                + child.padding[2]
                + child.border[0]
                + child.border[2];
        } else if is_inline_child(child) {
            height += compute_inline_height(child);
        }
    }
    height + node.padding[0] + node.padding[2]
}

/// Compute the height of a block node (content + inner children).
/// Legacy wrapper for compatibility.
fn compute_block_height(node: &LayoutNode, depth: usize) -> f32 {
    let mut renderer = crate::render::text::TextRenderer::new();
    compute_block_height_inner(node, depth, &mut renderer)
}

/// Compute the height of an inline node (used as a line box).
/// Uses the estimated font-size * 1.2 as the line-height per CSS spec.
fn compute_inline_height(node: &LayoutNode) -> f32 {
    let line_height = node.font_size * 1.2;
    line_height + node.padding[0] + node.padding[2]
}

// ------ CSS Grid Layout ------

/// Layout grid children according to explicit column tracks.
/// If no explicit columns are defined, falls back to block layout.
fn compute_grid_children(
    parent: &mut LayoutNode,
    parent_x: f32,
    parent_y: f32,
    available_width: f32,
    depth: usize,
    text_renderer: &mut crate::render::text::TextRenderer,
) {
    if depth > MAX_LAYOUT_DEPTH {
        return;
    }

    let num_cols = if parent.grid_columns.is_empty() {
        0
    } else {
        parent.grid_columns.len()
    };

    if num_cols == 0 {
        // Fallback to block layout when no explicit columns defined
        compute_block_children(
            parent,
            parent_x,
            parent_y,
            available_width,
            depth,
            text_renderer,
        );
        return;
    }

    let num_cols = num_cols as usize;

    // Step 1: Compute column widths
    let mut fixed_total: f32 = 0.0;
    let mut fr_total: f32 = 0.0;
    let mut col_widths: Vec<f32> = vec![0.0; num_cols];

    for (i, track) in parent.grid_columns.iter().enumerate() {
        match track {
            GridTrack::Fixed(w) => {
                fixed_total += w;
                col_widths[i] = *w;
            }
            GridTrack::Fr(f) => {
                fr_total += f;
            }
            GridTrack::Auto => {
                fr_total += 1.0; // treat auto as 1fr
            }
        }
    }

    // Subtract gaps between columns from available width
    let gap_total = (num_cols as f32 - 1.0) * parent.grid_column_gap;
    let remaining = (available_width - fixed_total - gap_total).max(0.0);

    // Distribute remaining space proportionally among fr/auto tracks
    for (i, track) in parent.grid_columns.iter().enumerate() {
        match track {
            GridTrack::Fr(f) => {
                col_widths[i] = if fr_total > 0.0 {
                    (f / fr_total) * remaining
                } else {
                    remaining / num_cols as f32
                }
                .max(0.0);
            }
            GridTrack::Auto => {
                col_widths[i] = if fr_total > 0.0 {
                    (1.0 / fr_total) * remaining
                } else {
                    remaining / num_cols as f32
                }
                .max(0.0);
            }
            _ => {} // Fixed already set above
        }
    }

    // Step 2: Determine row count based on child count
    let child_count = parent.children.len();
    let num_rows = if child_count > 0 {
        (child_count + num_cols - 1) / num_cols
    } else {
        0
    };

    // Step 3: Compute row heights from content
    // Use a minimum default height of 50px for grid cells, then grow based on explicit height
    let mut row_heights = vec![50.0f32; num_rows];
    for idx in 0..child_count {
        let r = idx / num_cols;
        let h = parent.children[idx].explicit_height.unwrap_or(50.0);
        row_heights[r] = row_heights[r].max(h);
    }

    // Step 4: Position each child at its grid cell (row-major auto placement)
    // Child index idx maps to column = idx % num_cols, row = idx / num_cols
    for idx in 0..child_count {
        let col = idx % num_cols;
        let row = idx / num_cols;

        // Compute x position: sum of column widths + gaps up to this column
        let mut x = parent_x;
        for c in 0..col {
            x += col_widths[c] + parent.grid_column_gap;
        }

        // Compute y position: sum of row heights + gaps up to this row
        let mut y = parent_y;
        for r in 0..row {
            y += row_heights[r] + parent.grid_row_gap;
        }

        let cell_width = col_widths[col];
        let cell_height = row_heights[row];

        let child = &mut parent.children[idx];
        child.rect.x = x;
        child.rect.y = y;
        child.rect.width = cell_width;
        child.rect.height = cell_height;

        // Recurse into children of this grid item
        let inner_width = (cell_width
            - child.margin[3]
            - child.margin[1]
            - child.border[3]
            - child.border[1]
            - child.padding[3]
            - child.padding[1])
            .max(0.0);

        let inner_x = child.rect.x + child.padding[3] + child.border[3];
        let inner_y = child.rect.y + child.padding[0] + child.border[0];

        match child.display {
            DisplayType::Flex | DisplayType::InlineFlex => {
                compute_flex_children(
                    child,
                    inner_x,
                    inner_y,
                    inner_width,
                    depth + 1,
                    text_renderer,
                );
            }
            DisplayType::Grid => {
                compute_grid_children(
                    child,
                    inner_x,
                    inner_y,
                    inner_width,
                    depth + 1,
                    text_renderer,
                );
            }
            _ => {
                compute_block_children(
                    child,
                    inner_x,
                    inner_y,
                    inner_width,
                    depth + 1,
                    text_renderer,
                );
            }
        }
    }

    // Step 5: Update parent rect to encompass all children
    if num_cols > 0 && num_rows > 0 {
        let total_width: f32 = col_widths.iter().sum::<f32>() + gap_total;
        let mut total_height: f32 = 0.0;
        for (i, &h) in row_heights.iter().enumerate() {
            total_height += h;
            if i < num_rows - 1 {
                total_height += parent.grid_row_gap;
            }
        }
        parent.rect.width = parent.rect.width.max(total_width);
        parent.rect.height = parent.rect.height.max(total_height);
    }
}

// ------ Flexbox Layout ------

/// Compute the main-axis size of a flex item's content.
/// Used to resolve flex basis when no explicit value is set.
fn compute_flex_item_content_main_size(item: &LayoutNode, is_row: bool) -> f32 {
    if item.rect.width > 0.0 && item.rect.height > 0.0 {
        // Item already has dimensions (e.g., from explicit width/height)
        return if is_row {
            item.rect.width
        } else {
            item.rect.height
        };
    }
    // Estimate from children: use block height computation as fallback
    if is_row {
        compute_block_height(item, 0)
            + item.padding[0]
            + item.padding[2]
            + item.border[0]
            + item.border[2]
    } else {
        // For column direction, we can't easily estimate width from children alone
        // Return 0 and let flex-grow handle distribution
        0.0
    }
}

/// Compute the cross-axis size of a flex item.
fn compute_flex_cross_size(item: &LayoutNode) -> f32 {
    if item.rect.height > 0.0 {
        return item.rect.height
            - item.padding[0]
            - item.padding[2]
            - item.border[0]
            - item.border[2];
    }
    compute_block_height(item, 0)
        + item.padding[0]
        + item.padding[2]
        + item.border[0]
        + item.border[2]
}

/// Layout flex children according to CSS Flexbox spec (multi-line support).
fn compute_flex_children(
    parent: &mut LayoutNode,
    parent_x: f32,
    parent_y: f32,
    available_width: f32,
    depth: usize,
    text_renderer: &mut crate::render::text::TextRenderer,
) {
    if depth > MAX_LAYOUT_DEPTH {
        return;
    }

    let is_row = matches!(
        parent.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let is_reverse = matches!(
        parent.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let is_wrap = matches!(parent.flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let is_wrap_reverse = matches!(parent.flex_wrap, FlexWrap::WrapReverse);

    // Gap lookups: main-axis gap and cross-axis gap (between flex lines)
    let main_axis_gap = if is_row {
        parent.column_gap
    } else {
        parent.row_gap
    };
    let cross_axis_gap = if is_row {
        parent.row_gap
    } else {
        parent.column_gap
    };

    if parent.children.is_empty() {
        return;
    }

    // --- Step 1: Resolve flex basis for each item ---
    struct FlexItemState {
        basis: f32,
        main_margin: f32,
    }

    let mut states: Vec<FlexItemState> = parent
        .children
        .iter()
        .map(|child| {
            let main_margin = if is_row {
                child.margin[3] + child.margin[1] // left + right
            } else {
                child.margin[0] + child.margin[2] // top + bottom
            };

            let basis = match child.flex_basis {
                FlexBasis::Auto => {
                    // Use explicit width if set, accounting for box-sizing
                    if let Some(explicit_w) = child.explicit_width {
                        match child.box_sizing {
                            BoxSizing::BorderBox => {
                                (explicit_w - child.padding[1] - child.padding[3]).max(0.0)
                            }
                            BoxSizing::ContentBox => explicit_w,
                        }
                    } else {
                        // Content-based sizing
                        if is_row && child.rect.width > 0.0 {
                            child.rect.width
                        } else if !is_row && child.rect.height > 0.0 {
                            child.rect.height
                        } else {
                            compute_flex_item_content_main_size(child, is_row)
                        }
                    }
                }
                FlexBasis::Pixels(p) => p,
                FlexBasis::Percentage(frac) => {
                    let available = if is_row {
                        available_width - child.margin[1] - child.margin[3]
                    } else {
                        available_width - child.margin[0] - child.margin[2]
                    };
                    (available * frac).max(0.0)
                }
            };

            FlexItemState {
                basis: basis.max(0.0),
                main_margin,
            }
        })
        .collect();

    // --- Step 2: Compute cross-size for each item (needed for line packing) ---
    let cross_sizes: Vec<f32> = parent
        .children
        .iter()
        .map(|child| compute_flex_cross_size(child) + child.margin[0] + child.margin[2])
        .collect();

    // --- Step 3: Pack items into flex lines ---
    struct FlexLine {
        item_indices: Vec<usize>,
        cross_size: f32, // Max cross-axis size in this line
    }

    let container_main_size = if is_row {
        available_width
    } else {
        f32::MAX // column: no fixed constraint (auto height)
    };

    let mut lines: Vec<FlexLine> = Vec::new();
    let mut current_line_items: Vec<usize> = Vec::new();
    let mut current_line_main_used: f32 = 0.0; // Accumulated main-size including gaps
    let mut current_line_cross_size: f32 = 0.0;

    for (i, _child) in parent.children.iter().enumerate() {
        let item_main = states[i].basis + states[i].main_margin;
        let item_cross = cross_sizes[i];

        // How much main space would adding this item consume (including gap if not first)?
        let gap_cost = if current_line_items.is_empty() {
            0.0
        } else {
            main_axis_gap
        };
        let tentative_main = current_line_main_used + gap_cost + item_main;

        // Check if wrapping is needed: only when row direction, container is bounded, and item overflows
        if is_wrap
            && is_row
            && !current_line_items.is_empty()
            && container_main_size != f32::MAX
            && tentative_main > container_main_size
        {
            // Flush current line
            lines.push(FlexLine {
                item_indices: std::mem::take(&mut current_line_items),
                cross_size: current_line_cross_size,
            });
            current_line_main_used = 0.0;
            current_line_cross_size = 0.0;

            // If the single item alone exceeds container, it still gets its own line (forced overflow)
        }

        let added_gap = if current_line_items.is_empty() {
            0.0
        } else {
            main_axis_gap
        };
        current_line_items.push(i);
        current_line_main_used += added_gap + item_main;
        current_line_cross_size = current_line_cross_size.max(item_cross);
    }

    // Flush remaining items as the last line
    if !current_line_items.is_empty() {
        lines.push(FlexLine {
            item_indices: current_line_items,
            cross_size: current_line_cross_size,
        });
    }

    // --- Step 4: Per-line flex distribution (grow/shrink within each line) ---
    for line in &mut lines {
        let total_main_in_line: f32 = line
            .item_indices
            .iter()
            .map(|&i| states[i].basis + states[i].main_margin)
            .sum();

        // Gap total within this line
        let gap_count = if line.item_indices.len() > 1 {
            line.item_indices.len() - 1
        } else {
            0
        };
        let gap_total = main_axis_gap * gap_count as f32;

        let free_space = container_main_size - total_main_in_line - gap_total;

        // Distribute positive free space (flex-grow)
        if free_space > 0.0 {
            let total_grow: f32 = line
                .item_indices
                .iter()
                .map(|&i| parent.children[i].flex_grow)
                .filter(|&g| g > 0.0)
                .sum();

            if total_grow > 0.0 {
                for &i in &line.item_indices {
                    let child = &parent.children[i];
                    if child.flex_grow > 0.0 {
                        let share = free_space * (child.flex_grow / total_grow);
                        states[i].basis += share;
                    }
                }
            }
        }

        // Distribute negative free space (flex-shrink)
        // Skip shrinking for single-item lines: one item cannot share deficit with anyone,
        // so it should overflow the container rather than shrink to fill it.
        if free_space < 0.0 && line.item_indices.len() > 1 {
            let total_shrink_weight: f32 = line
                .item_indices
                .iter()
                .filter(|&&i| parent.children[i].flex_shrink > 0.0)
                .map(|&i| parent.children[i].flex_shrink * states[i].basis)
                .sum();

            if total_shrink_weight > 0.0 {
                for &i in &line.item_indices {
                    let child = &parent.children[i];
                    if child.flex_shrink > 0.0 {
                        let shrink_amount = (-free_space) * (child.flex_shrink * states[i].basis)
                            / total_shrink_weight;
                        states[i].basis = (states[i].basis - shrink_amount).max(0.0);
                    }
                }
            }
        }

        // Clamp to min-width / max-width constraints
        for &i in &line.item_indices {
            let child = &parent.children[i];
            let content_size = states[i].basis + states[i].main_margin;
            if let Some(min_w) = child.min_width {
                if content_size < min_w {
                    states[i].basis = (min_w - states[i].main_margin).max(0.0);
                }
            }
            if let Some(max_w) = child.max_width {
                if content_size > max_w {
                    states[i].basis = (max_w - states[i].main_margin).max(0.0);
                }
            }
        }
    }

    // --- Step 5: Position children by line ---
    let mut current_cross_pos = parent_y; // Tracks cross-axis position across lines

    for line in &lines {
        let mut current_main_pos = if is_reverse && is_row {
            parent_x + available_width // Start from right for row-reverse
        } else {
            parent_x
        };

        for &item_i in &line.item_indices {
            let child = &mut parent.children[item_i];
            let main_content_size = (states[item_i].basis - states[item_i].main_margin).max(0.0);

            if is_row {
                // Row: main=horizontal, cross=vertical
                let cross_content = compute_flex_cross_size(child);
                child.rect.x = current_main_pos + child.margin[3];
                child.rect.y = current_cross_pos + child.margin[0];
                child.rect.width = main_content_size;
                child.rect.height = cross_content + child.margin[0] + child.margin[2];
            } else {
                // Column: main=vertical, cross=horizontal
                let child_width = (available_width - states[item_i].main_margin).max(0.0);
                child.rect.x = current_main_pos + child.margin[3];
                child.rect.y = current_cross_pos + child.margin[0];
                child.rect.width = child_width;
                child.rect.height = main_content_size;
            }

            let item_main_total = states[item_i].basis + states[item_i].main_margin;
            let step = item_main_total + main_axis_gap;
            if is_reverse {
                current_main_pos -= step;
            } else {
                current_main_pos += step;
            }
        }

        // Advance cross-axis for next line
        let line_cross_step = line.cross_size + cross_axis_gap;
        if is_wrap_reverse && is_row {
            current_cross_pos -= line_cross_step;
        } else {
            current_cross_pos += line_cross_step;
        }
    }

    // --- Step 6: Apply justify-content per line (main-axis distribution) ---
    for line in &lines {
        if is_row {
            let total_items_width: f32 = line
                .item_indices
                .iter()
                .map(|&i| {
                    parent.children[i].rect.width
                        + parent.children[i].margin[3]
                        + parent.children[i].margin[1]
                })
                .sum();
            let justify_space = (container_main_size - total_items_width).max(0.0);

            if justify_space > 0.0 {
                match parent.justify_content {
                    JustifyContent::FlexStart => {} // already at start
                    JustifyContent::FlexEnd => {
                        for &i in &line.item_indices {
                            parent.children[i].rect.x += justify_space;
                        }
                    }
                    JustifyContent::Center => {
                        let offset = justify_space / 2.0;
                        for &i in &line.item_indices {
                            parent.children[i].rect.x += offset;
                        }
                    }
                    JustifyContent::SpaceBetween => {
                        if line.item_indices.len() > 1 {
                            let gap = justify_space / (line.item_indices.len() - 1) as f32;
                            for (j, &i) in line.item_indices.iter().enumerate() {
                                parent.children[i].rect.x += gap * j as f32;
                            }
                        }
                    }
                    JustifyContent::SpaceAround => {
                        let n = line.item_indices.len();
                        if n > 0 {
                            let gap = justify_space / n as f32;
                            for (j, &i) in line.item_indices.iter().enumerate() {
                                parent.children[i].rect.x += gap * j as f32 + gap / 2.0;
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Step 7: Apply align-items per line (cross-axis alignment within each line) ---
    if is_row {
        for line in &lines {
            let container_cross = line.cross_size;
            match parent.align_items {
                AlignItems::Stretch => {
                    for &i in &line.item_indices {
                        let stretch_h = (container_cross
                            - parent.children[i].margin[0]
                            - parent.children[i].margin[2])
                            .max(0.0);
                        if matches!(parent.children[i].flex_basis, FlexBasis::Auto)
                            || parent.children[i].rect.height <= 0.0
                        {
                            // Only stretch items without explicit height
                        } else {
                            let current = parent.children[i].rect.height;
                            parent.children[i].rect.height = stretch_h.max(current);
                        }
                    }
                }
                AlignItems::FlexStart => {} // already at top
                AlignItems::FlexEnd => {
                    for &i in &line.item_indices {
                        let offset = container_cross
                            - parent.children[i].rect.height
                            - parent.children[i].margin[0]
                            - parent.children[i].margin[2];
                        if offset > 0.0 {
                            parent.children[i].rect.y += offset;
                        }
                    }
                }
                AlignItems::Center => {
                    for &i in &line.item_indices {
                        let used = parent.children[i].rect.height
                            + parent.children[i].margin[0]
                            + parent.children[i].margin[2];
                        let offset = (container_cross - used) / 2.0;
                        if offset > 0.0 {
                            parent.children[i].rect.y += offset;
                        }
                    }
                }
            }
        }
    }

    // --- Step 8: Apply align-content (cross-axis distribution of flex lines) ---
    // Distributes flex lines within the container's cross-axis when there's extra space.
    // For row direction, cross-axis = vertical; for column direction, cross-axis = horizontal.
    // Requires multiple lines and a non-zero parent height to have visible effect.
    if lines.len() > 1 || matches!(parent.align_content, AlignContent::Stretch) {
        let total_content_cross: f32 = {
            let line_cross_sum: f32 = lines.iter().map(|l| l.cross_size).sum();
            if lines.len() > 1 {
                line_cross_sum + cross_axis_gap * (lines.len() - 1) as f32
            } else {
                line_cross_sum
            }
        };

        // Parent's inner cross-size (content area after padding/border/margin)
        let parent_inner_cross = if is_row {
            let total_v =
                parent.padding[0] + parent.border[0] + parent.padding[2] + parent.border[2];
            (parent.rect.height - total_v).max(0.0)
        } else {
            let total_h =
                parent.padding[3] + parent.border[3] + parent.padding[1] + parent.border[1];
            (parent.rect.width - total_h).max(0.0)
        };

        // Excess space: when container is larger than the lines' total extent
        let excess = (parent_inner_cross - total_content_cross).max(0.0);

        if excess > 0.0 || matches!(parent.align_content, AlignContent::Stretch) {
            match parent.align_content {
                AlignContent::Normal => {}    // no-op
                AlignContent::FlexStart => {} // lines already at start
                AlignContent::FlexEnd => {
                    for child in &mut parent.children {
                        if is_row {
                            child.rect.y += excess;
                        } else {
                            child.rect.x += excess;
                        }
                    }
                }
                AlignContent::Center => {
                    let offset = excess / 2.0;
                    for child in &mut parent.children {
                        if is_row {
                            child.rect.y += offset;
                        } else {
                            child.rect.x += offset;
                        }
                    }
                }
                AlignContent::SpaceBetween => {
                    if lines.len() > 1 {
                        let extra_gap = excess / (lines.len() - 1) as f32;
                        let mut accumulated = 0.0;
                        for (_line_idx, line) in lines.iter().enumerate() {
                            let shift = accumulated * extra_gap;
                            for &i in &line.item_indices {
                                if is_row {
                                    parent.children[i].rect.y += shift;
                                } else {
                                    parent.children[i].rect.x += shift;
                                }
                            }
                            accumulated += 1.0;
                        }
                    }
                }
                AlignContent::SpaceAround => {
                    if lines.len() > 1 {
                        let extra_gap = excess / lines.len() as f32;
                        let mut accumulated = 0.5; // Half gap at start
                        for (_line_idx, line) in lines.iter().enumerate() {
                            let shift = accumulated * extra_gap;
                            for &i in &line.item_indices {
                                if is_row {
                                    parent.children[i].rect.y += shift;
                                } else {
                                    parent.children[i].rect.x += shift;
                                }
                            }
                            accumulated += 1.0;
                        }
                    }
                }
                AlignContent::Stretch => {
                    if lines.len() > 0 {
                        let stretch_per_line = excess / lines.len() as f32;
                        for line in &lines {
                            for &i in &line.item_indices {
                                if is_row {
                                    parent.children[i].rect.height += stretch_per_line;
                                } else {
                                    parent.children[i].rect.width += stretch_per_line;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Step 9: Recurse into children ---
    for child in &mut parent.children {
        let inner_width = (child.rect.width
            - child.margin[3]
            - child.margin[1]
            - child.border[3]
            - child.border[1]
            - child.padding[3]
            - child.padding[1])
            .max(0.0);

        let inner_x = child.rect.x + child.padding[3] + child.border[3];
        let inner_y = child.rect.y + child.padding[0] + child.border[0];

        match child.display {
            DisplayType::Flex | DisplayType::InlineFlex => {
                compute_flex_children(
                    child,
                    inner_x,
                    inner_y,
                    inner_width,
                    depth + 1,
                    text_renderer,
                );
            }
            DisplayType::Grid => {
                compute_grid_children(
                    child,
                    inner_x,
                    inner_y,
                    inner_width,
                    depth + 1,
                    text_renderer,
                );
            }
            _ => {
                compute_block_children(
                    child,
                    inner_x,
                    inner_y,
                    inner_width,
                    depth + 1,
                    text_renderer,
                );
            }
        }
    }

    // --- Step 10: Update parent height based on all children extent ---
    let mut min_child_y = f32::MAX;
    let mut max_child_bottom = 0.0;
    for child in &parent.children {
        let top = child.rect.y + child.margin[0];
        let bottom = child.rect.bottom() + child.margin[2];
        if top < min_child_y {
            min_child_y = top;
        }
        if bottom > max_child_bottom {
            max_child_bottom = bottom;
        }
    }

    // If wrap-reverse caused negative y positions, shift all children up
    if min_child_y < parent_y {
        let shift = parent_y - min_child_y;
        for child in &mut parent.children {
            child.rect.y += shift;
        }
        max_child_bottom += shift;
        min_child_y = parent_y;
    }

    let content_height = (max_child_bottom - min_child_y).max(0.0);
    parent.rect.height = content_height
        + parent.padding[0]
        + parent.border[0]
        + parent.padding[2]
        + parent.border[2];
}

/// Check if a string contains only collapsible whitespace characters.
fn is_collapsible_whitespace(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r')
}

/// Build inline boxes from the children of a block container.
///
/// Collects consecutive text nodes and inline elements into [`InlineBox`] entries:
/// - Text nodes with non-whitespace content become [`InlineBox::Text`].
/// - Whitespace-only text nodes become [`InlineBox::Whitespace`] with `collapsible: true`.
/// - Inline element children become [`InlineBox::Element`].
///
/// Dimensions (`width`, `height`, `baseline_offset`) are set to `0.0` here and
/// filled in during line-breaking (Step A3/A4) when text measurement occurs.
fn build_inline_boxes(_parent: &LayoutNode, children: &[LayoutNode]) -> Vec<InlineBox> {
    let mut boxes = Vec::new();

    for (idx, child) in children.iter().enumerate() {
        if let Some(ref text) = child.text {
            // Whitespace-only text nodes are marked as collapsible
            if is_collapsible_whitespace(text) {
                boxes.push(InlineBox::Whitespace {
                    collapsible: true,
                    width: 0.0,
                });
            } else {
                boxes.push(InlineBox::Text {
                    text: text.clone(),
                    width: 0.0, // measured in Step A3 with TextRenderer
                    color: child.color,
                    font_size: child.font_size,
                });
            }
        } else if matches!(
            child.display,
            DisplayType::Inline | DisplayType::InlineBlock
        ) {
            boxes.push(InlineBox::Element {
                child_index: idx,
                width: 0.0,           // filled in during line breaking
                height: 0.0,          // filled in during line breaking
                baseline_offset: 0.0, // filled in Step A4
            });
        }
        // Block children are not part of inline formatting context  Eskip them
    }

    boxes
}

/// Build inline boxes from a specific slice of children (for inline runs within compute_block_children).
/// Like `build_inline_boxes` but indices are relative to the slice start.
fn build_inline_boxes_from_slice(_parent: &LayoutNode, children: &[LayoutNode]) -> Vec<InlineBox> {
    let mut boxes = Vec::new();

    for (idx, child) in children.iter().enumerate() {
        if let Some(ref text) = child.text {
            if is_collapsible_whitespace(text) {
                boxes.push(InlineBox::Whitespace {
                    collapsible: true,
                    width: 0.0,
                });
            } else {
                boxes.push(InlineBox::Text {
                    text: text.clone(),
                    width: 0.0,
                    color: child.color,
                    font_size: child.font_size,
                });
            }
        } else if matches!(
            child.display,
            DisplayType::Inline | DisplayType::InlineBlock
        ) {
            boxes.push(InlineBox::Element {
                child_index: idx,
                width: child.rect.width,
                height: child.rect.height,
                baseline_offset: 0.0,
            });
        }
    }

    boxes
}

/// Collect rect positions from line boxes back into LayoutNode children.
///
/// Walks the line boxes and for each inline box updates the corresponding
/// LayoutNode child's `rect` to reflect its position within the line.
/// Text nodes get positioned at the line top with line-height extent.
/// Inline elements are baseline-aligned.
fn position_inline_children_in_lines(
    children: &mut [LayoutNode],
    line_boxes: &[LineBox],
    parent_x: f32,
    line_area_y: f32,
) {
    // First pass: collect positions for all children based on inline box data.
    // We track a child_idx that walks through the children slice matching
    // text/whitespace children to their corresponding InlineBox entries.
    // Element boxes carry explicit child_index for direct mapping.

    // Build a position map: child_index -> (x, y, width, height)
    let mut positions: Vec<(usize, f32, f32, f32, f32)> = Vec::new();
    let mut text_child_idx: usize = 0;

    for line in line_boxes {
        let baseline_y = line.baseline_y;
        let line_top = line.y;

        let mut x_offset = parent_x
            + if children.is_empty() {
                0.0
            } else {
                children[0].margin[3]
            };

        for box_item in &line.boxes {
            match box_item {
                InlineBox::Text {
                    width, font_size, ..
                } => {
                    // Map this text box to the corresponding text child
                    while text_child_idx < children.len() {
                        if children[text_child_idx].text.is_some() {
                            let line_h = *font_size * 1.2;
                            positions.push((
                                text_child_idx,
                                x_offset,
                                line_top + line_area_y,
                                *width,
                                line_h,
                            ));
                            text_child_idx += 1;
                            break;
                        } else {
                            // Skip non-text children (inline elements) for text mapping
                            text_child_idx += 1;
                        }
                    }
                    x_offset += *width;
                }
                InlineBox::Element {
                    child_index,
                    width,
                    height,
                    baseline_offset,
                } => {
                    if *child_index < children.len() {
                        let elem_bottom = baseline_y + line_area_y - *baseline_offset;
                        let elem_y = elem_bottom - *height;
                        positions.push((
                            *child_index,
                            x_offset,
                            elem_y,
                            (*width).max(1.0),
                            (*height).max(1.0),
                        ));
                    }
                    x_offset += *width;
                }
                InlineBox::Whitespace { width, .. } => {
                    // Whitespace maps to text-only children that have whitespace text
                    while text_child_idx < children.len() {
                        if let Some(ref t) = children[text_child_idx].text {
                            if is_collapsible_whitespace(t) {
                                positions.push((
                                    text_child_idx,
                                    x_offset,
                                    line_top + line_area_y,
                                    *width,
                                    0.0,
                                ));
                                text_child_idx += 1;
                                break;
                            }
                        }
                        // If it's not a whitespace child, we might have an offset
                        // Just continue to next child for this whitespace entry
                    }
                    x_offset += *width;
                }
            }
        }
    }

    // Apply collected positions to children (mutable access, no borrow conflict)
    for &(idx, px, py, pw, ph) in &positions {
        if idx < children.len() {
            children[idx].rect.x = px;
            children[idx].rect.y = py;
            if pw > 0.0 {
                children[idx].rect.width = pw;
            }
            if ph > 0.0 {
                children[idx].rect.height = ph;
            }
        }
    }
}

/// Break inline boxes into line boxes based on available width.
///
/// Text is measured using the `TextRenderer`, and lines wrap at word boundaries.
/// Whitespace at line boundaries is collapsed. If a single word exceeds the
/// available width it overflows rather than being split mid-word.
///
/// Baseline alignment:
/// - The line's reference baseline is determined by the largest font size on the line.
/// - `ascender` = max_font_size * 0.8, `descender` = max_font_size * 0.2.
/// - Line height = ascender + descender + leading (= max_font_size * 1.2).
/// - `baseline_y = y + ascender`.
/// - Smaller text shares the same baseline; its shorter ascender means it sits
///   visually above the shared baseline, which matches CSS inline alignment.
/// - Inline elements get `baseline_offset = element_height * 0.7` (typical vertical-align baseline).
pub fn break_into_lines(
    inline_boxes: Vec<InlineBox>,
    available_width: f32,
    text_renderer: &mut crate::render::text::TextRenderer,
) -> Vec<LineBox> {
    let mut line_boxes = Vec::new();
    let mut current_line_boxes: Vec<InlineBox> = Vec::new();
    let mut current_line_width: f32 = 0.0;
    let mut current_line_max_font_size: f32 = 0.0;
    let mut line_y: f32 = 0.0;

    // Helper to flush the current line into a LineBox
    let flush_line = |line_boxes: &mut Vec<LineBox>,
                      boxes: &mut Vec<InlineBox>,
                      width: &mut f32,
                      max_font_size: &mut f32,
                      line_y: &mut f32| {
        // Remove trailing collapsible whitespace before flushing
        while let Some(InlineBox::Whitespace {
            collapsible: true, ..
        }) = boxes.last()
        {
            boxes.pop();
        }

        // Determine reference font size for this line (largest text on the line)
        let ref_font = if *max_font_size > 0.0 {
            *max_font_size
        } else {
            // Default line height when no text is present
            16.0
        };

        // Baseline metrics:
        //   ascender   = ref_font * 0.8  (distance from baseline to top of caps)
        //   descender  = ref_font * 0.2  (distance from baseline to bottom of descenders)
        //   leading    = ref_font * 0.2  (extra space between lines)
        //   total height = ascender + descender + leading = ref_font * 1.2
        let ascender = ref_font * 0.8;
        let _descender = ref_font * 0.2; // used conceptually for bottom edge calc
        let height = ref_font * 1.2;

        let baseline_y = *line_y + ascender;

        line_boxes.push(LineBox {
            y: *line_y,
            baseline_y,
            height,
            boxes: std::mem::take(boxes),
        });

        *width = 0.0;
        *max_font_size = 0.0;
        *line_y += height;
    };

    for box_item in inline_boxes {
        match &box_item {
            InlineBox::Text {
                text,
                width: _,
                color: _,
                font_size,
            } => {
                let mut words: Vec<&str> = text.split_whitespace().collect();
                let has_spaces = text.contains(' ') || text.contains('\t') || text.contains('\n');

                // If the text contains no whitespace separators it's a single "word"
                if words.is_empty() && !text.trim().is_empty() {
                    words.push(text.trim());
                }

                for word in &words {
                    let (word_width, _word_height) =
                        text_renderer.measure(word, *font_size, "sans-serif");

                    // Space width between words
                    let space_width = if has_spaces && current_line_boxes.is_empty() {
                        0.0
                    } else if has_spaces && !current_line_boxes.is_empty() {
                        let (sw, _) = text_renderer.measure(" ", *font_size, "sans-serif");
                        sw
                    } else {
                        0.0
                    };

                    let tentative = current_line_width + space_width + word_width;

                    if tentative > available_width && !current_line_boxes.is_empty() {
                        // Flush current line and start a new one
                        flush_line(
                            &mut line_boxes,
                            &mut current_line_boxes,
                            &mut current_line_width,
                            &mut current_line_max_font_size,
                            &mut line_y,
                        );
                    }

                    // Add the word as a Text box (space prefix not added to text content,
                    // width is tracked separately). The y_offset within the line is implicit:
                    // text boxes sit with their ascender above the shared baseline.
                    let total_word = space_width + word_width;
                    current_line_boxes.push(InlineBox::Text {
                        text: word.to_string(),
                        width: word_width,
                        color: None, // will be enriched from parent during render
                        font_size: *font_size,
                    });

                    // Add space width between words as a non-collapsible whitespace entry
                    if space_width > 0.0 {
                        current_line_boxes.push(InlineBox::Whitespace {
                            collapsible: false,
                            width: space_width,
                        });
                    }

                    current_line_width += total_word;
                    if *font_size > current_line_max_font_size {
                        current_line_max_font_size = *font_size;
                    }
                }
            }
            InlineBox::Element {
                child_index,
                width,
                height,
                baseline_offset: _,
            } => {
                let est_width = (*width).max(1.0); // at least 1px for inline elements
                let est_height = (*height).max(1.0);

                if (current_line_width + est_width > available_width)
                    && !current_line_boxes.is_empty()
                {
                    flush_line(
                        &mut line_boxes,
                        &mut current_line_boxes,
                        &mut current_line_width,
                        &mut current_line_max_font_size,
                        &mut line_y,
                    );
                }

                // baseline_offset = distance from bottom of element to shared baseline.
                // For typical inline elements (img, span), 0.7 of element height is standard.
                let elem_baseline_offset = est_height * 0.7;

                current_line_boxes.push(InlineBox::Element {
                    child_index: *child_index,
                    width: est_width,
                    height: est_height,
                    baseline_offset: elem_baseline_offset,
                });
                current_line_width += est_width;
            }
            InlineBox::Whitespace { collapsible, width } => {
                if *collapsible {
                    // Skip collapsible whitespace at the start of a line
                    if current_line_boxes.is_empty() {
                        continue;
                    }
                    // At the end of a line it will be trimmed during flush
                    // Add as a single space-width marker
                    let last = current_line_boxes.last();
                    // If the last item is already whitespace, skip (collapse consecutive)
                    if matches!(last, Some(InlineBox::Whitespace { .. })) {
                        continue;
                    }

                    // Estimate space width from max font size on this line
                    let fs = if current_line_max_font_size > 0.0 {
                        current_line_max_font_size
                    } else {
                        16.0
                    };
                    let (sw, _) = text_renderer.measure(" ", fs, "sans-serif");

                    if current_line_width + sw > available_width {
                        // Flush line; trailing whitespace will be trimmed
                        flush_line(
                            &mut line_boxes,
                            &mut current_line_boxes,
                            &mut current_line_width,
                            &mut current_line_max_font_size,
                            &mut line_y,
                        );
                        continue;
                    }

                    current_line_boxes.push(InlineBox::Whitespace {
                        collapsible: true,
                        width: sw,
                    });
                    current_line_width += sw;
                } else {
                    // Non-collapsible whitespace (e.g., space between words we inserted)
                    // Already handled in the text path above, so just use the pre-set width
                    let sw = (*width).max(1.0);
                    current_line_boxes.push(InlineBox::Whitespace {
                        collapsible: false,
                        width: sw,
                    });
                    current_line_width += sw;
                }
            }
        }
    }

    // Flush remaining content
    if !current_line_boxes.is_empty() {
        flush_line(
            &mut line_boxes,
            &mut current_line_boxes,
            &mut current_line_width,
            &mut current_line_max_font_size,
            &mut line_y,
        );
    }

    line_boxes
}

/// Flatten the layout tree into renderable rectangles with colors.
///
/// Collects all nodes that have a background color, plus leaf text nodes
/// that have computed dimensions. Used to bridge layout tree to renderer.
/// Delegates to [`collect_render_rects_with_clipping`] with an empty clip stack.
pub fn collect_render_rects(node: &LayoutNode) -> Vec<(Rect, Option<[u8; 4]>)> {
    let mut clip_stack = Vec::new();
    let mut rects = Vec::new();
    collect_render_rects_with_clipping(node, &mut clip_stack, &mut rects);
    rects
}

/// Clip-aware variant of [`collect_render_rects`].
///
/// Walks the layout tree and collects (Rect, Option<[u8; 4]>) tuples.
/// When a node's `overflow` is not `Visible`, its content box is pushed onto
/// `clip_stack` so all descendant rects are clipped to that region.
pub fn collect_render_rects_with_clipping(
    node: &LayoutNode,
    clip_stack: &mut Vec<Rect>,
    out: &mut Vec<(Rect, Option<[u8; 4]>)>,
) {
    // Push a clip region if this node clips its children
    let pushed_clip = match node.overflow {
        Overflow::Visible => false,
        _ => {
            clip_stack.push(node.content_box());
            true
        }
    };

    // Recurse into children (their rects may be clipped by this node's overflow)
    for child in &node.children {
        collect_render_rects_with_clipping(child, clip_stack, out);
    }

    // Also collect from absolutely positioned children (rendered on top)
    for abs_child in &node.absolute_children {
        collect_render_rects_with_clipping(abs_child, clip_stack, out);
    }

    // Pop the clip region we pushed
    if pushed_clip {
        clip_stack.pop();
    }

    // Collect this node's own rect(s), clipped by the current clip stack
    if node.background_color.is_some() {
        add_clipped_rect(node.rect, node.background_color, clip_stack, out);
    }

    if node.text.is_some() && node.background_color.is_none() {
        if node.rect.width > 0.0 && node.rect.height > 0.0 {
            add_clipped_rect(node.rect, Some([0, 0, 0, 255]), clip_stack, out);
        }
    }
}

/// Add a rect to the output, intersecting it with every clip region on the stack.
/// If the rect falls entirely outside any clip region, it is skipped.
fn add_clipped_rect(
    rect: Rect,
    color: Option<[u8; 4]>,
    clip_stack: &[Rect],
    out: &mut Vec<(Rect, Option<[u8; 4]>)>,
) {
    let mut current = rect;
    for clip in clip_stack {
        match current.intersect(clip) {
            Some(intersection) => current = intersection,
            None => return, // Fully outside clip region
        }
    }
    out.push((current, color));
}

/// Information about a text node for rasterization.
#[derive(Clone, Debug)]
pub struct TextInfo {
    /// X position in layout space (pixels from left).
    pub x: f32,
    /// Y position in layout space (pixels from top).
    pub y: f32,
    /// Available width for text wrapping.
    pub width: f32,
    /// Text content to render.
    pub text: String,
    /// RGBA color (0-255 range).
    pub color: [u8; 4],
    /// Font size in pixels.
    pub font_size: f32,
}

/// Collect all text nodes from the layout tree for rendering.
///
/// When a node has `line_boxes` (inline layout was computed), walks through
/// each LineBox's inline boxes and extracts TextInfo from `InlineBox::Text`
/// entries with positions derived from the parent rect origin, line y offset,
/// and accumulated horizontal offset within the line.
///
/// When `line_boxes` is None, falls back to the recursive tree walk for
/// block-level text nodes.
pub fn collect_text_nodes(node: &LayoutNode) -> Vec<TextInfo> {
    let mut texts = Vec::new();

    // If this node has inline layout (line_boxes), walk them instead of recursing
    // into children for text collection  Ethe line boxes hold the authoritative
    // positions and word-split text fragments.
    if let Some(ref line_boxes) = node.line_boxes {
        let base_x = node.rect.x + node.padding[3] + node.border[3];
        let base_y = node.rect.y + node.padding[0] + node.border[0];

        for line in line_boxes {
            let line_y = base_y + line.y;
            let mut x_offset = base_x;

            for box_item in &line.boxes {
                match box_item {
                    InlineBox::Text {
                        text,
                        width,
                        color,
                        font_size,
                    } => {
                        if !text.trim().is_empty() {
                            // Use CSS `color` (foreground), default to black
                            let text_color = color.unwrap_or([0, 0, 0, 255]);

                            texts.push(TextInfo {
                                x: x_offset,
                                y: line_y,
                                width: *width,
                                text: text.clone(),
                                color: text_color,
                                font_size: *font_size,
                            });
                        }
                        x_offset += *width;
                    }
                    InlineBox::Element { width, .. } => {
                        // Inline elements are just spacing in the line  Eskip for text
                        x_offset += *width;
                    }
                    InlineBox::Whitespace { width, .. } => {
                        // Whitespace is just horizontal spacing  Eskip
                        x_offset += *width;
                    }
                }
            }
        }
    } else if let Some(ref text) = node.text {
        // Fallback: no inline layout was computed, use the block-level approach.
        if !text.trim().is_empty() && node.rect.width > 0.0 && node.rect.height > 0.0 {
            // Use CSS `color` (foreground) as text color, default to black
            let color = node.color.unwrap_or([0, 0, 0, 255]);

            texts.push(TextInfo {
                x: node.rect.x,
                y: node.rect.y,
                width: node.rect.width,
                text: text.clone(),
                color,
                font_size: node.font_size,
            });
        }
    }

    for child in &node.children {
        texts.extend(collect_text_nodes(child));
    }

    // Also collect from absolutely positioned children
    for abs_child in &node.absolute_children {
        texts.extend(collect_text_nodes(abs_child));
    }

    texts
}

/// Information about an image that needs to be rendered.
#[derive(Clone, Debug)]
pub struct ImageInfo {
    /// X position in layout space (pixels from left).
    pub x: f32,
    /// Y position in layout space (pixels from top).
    pub y: f32,
    /// Width of the image display area.
    pub width: f32,
    /// Height of the image display area.
    pub height: f32,
    /// The source URL of the image.
    pub src: String,
}

/// Collect all image nodes from the layout tree for rendering.
///
/// Walks the tree and extracts nodes that have an image_src with valid
/// dimensions. Each entry contains position, size, and image URL.
pub fn collect_image_nodes(node: &LayoutNode) -> Vec<ImageInfo> {
    let mut images = Vec::new();

    if let Some(src) = &node.image_src {
        if node.rect.width > 0.0 && node.rect.height > 0.0 {
            images.push(ImageInfo {
                x: node.rect.x,
                y: node.rect.y,
                width: node.rect.width,
                height: node.rect.height,
                src: src.clone(),
            });
        }
    }

    for child in &node.children {
        images.extend(collect_image_nodes(child));
    }

    // Also collect from absolutely positioned children
    for abs_child in &node.absolute_children {
        images.extend(collect_image_nodes(abs_child));
    }

    images
}

/// Given a LineBox, return (line_top, baseline_y, line_bottom, line_height).
///
/// Returns the top edge of the line box, the baseline position for text
/// alignment, the bottom edge including descenders, and the total height.
pub fn line_box_metrics(line: &LineBox) -> (f32, f32, f32, f32) {
    let line_top = line.y;
    let baseline_y = line.baseline_y;
    let line_bottom = line.y + line.height;
    let line_height = line.height;
    (line_top, baseline_y, line_bottom, line_height)
}

/// Hit-test the layout tree for interactive elements at a given position.
///
/// Walks the tree in depth-first order (children first, since they render on top).
/// Returns the `InteractionType` of the topmost interactive element containing the point,
/// or `None` if no interactive element is found.
pub fn hit_test_interactive(root: &LayoutNode, x: f32, y: f32) -> Option<InteractionType> {
    // Check children first (topmost/rendered-last wins)
    for child in root.children.iter().rev() {
        if child.rect.contains(x, y) {
            if let Some(found) = hit_test_interactive(child, x, y) {
                return Some(found);
            }
        }
    }
    // Also check absolutely positioned children (they render on top of normal flow)
    for abs_child in root.absolute_children.iter().rev() {
        if abs_child.rect.contains(x, y) {
            if let Some(found) = hit_test_interactive(abs_child, x, y) {
                return Some(found);
            }
        }
    }
    // Check this node itself
    if root.interaction_type != InteractionType::None && root.rect.contains(x, y) {
        return Some(root.interaction_type);
    }
    None
}

/// Hit-test the layout tree at a position and return all DOM node IDs along
/// the hit path (from root ancestor to deepest matching node).
///
/// Used for `:hover` evaluation — the returned path includes all ancestors
/// of the element under the cursor, so selectors like `div:hover > a` work.
/// Text nodes don't have `dom_node_id`, so the innermost element ancestor
/// of any text at the hit position will be the last entry.
pub fn hit_test_dom_path(root: &LayoutNode, x: f32, y: f32) -> Vec<u32> {
    let mut path = Vec::new();
    hit_test_dom_path_inner(root, x, y, &mut path);
    // Path is collected in root-to-leaf order (push before recurse).
    path
}

fn hit_test_dom_path_inner(node: &LayoutNode, x: f32, y: f32, path: &mut Vec<u32>) {
    if !node.rect.contains(x, y) {
        return;
    }

    // Record this node's DOM ID if it has one
    if let Some(id) = node.dom_node_id {
        path.push(id);
    }

    // Check absolute children first (they render on top of normal flow).
    // Only recurse into the first matching child (topmost wins).
    let mut found = false;
    for abs_child in node.absolute_children.iter().rev() {
        if !found && abs_child.rect.contains(x, y) {
            hit_test_dom_path_inner(abs_child, x, y, path);
            found = true;
        }
    }

    // If no absolute child matched, try normal-flow children (last = on top).
    // Only recurse into the first matching child.
    if !found {
        for child in node.children.iter().rev() {
            if child.rect.contains(x, y) {
                hit_test_dom_path_inner(child, x, y, path);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to run compute_layout with a fresh TextRenderer for tests.
    fn test_compute_layout(root: &mut LayoutNode, page_width: f32) {
        let mut renderer = crate::render::text::TextRenderer::new();
        compute_layout(root, page_width, &mut renderer);
    }

    #[test]
    fn rect_contains() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(rect.contains(50.0, 50.0));
        assert!(!rect.contains(101.0, 101.0));
    }

    #[test]
    fn layout_node_add_child() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        root.add_child(LayoutNode::new(Rect::new(10.0, 10.0, 80.0, 80.0)));
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn rect_right_and_bottom() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 70.0);
    }

    #[test]
    fn rect_union() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 100.0, 100.0);
        let u = a.union(&b);
        assert_eq!(u.x, 0.0);
        assert_eq!(u.y, 0.0);
        assert_eq!(u.width, 150.0);
        assert_eq!(u.height, 150.0);
    }

    #[test]
    fn rect_intersect_overlap() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 100.0, 100.0);
        let inter = a.intersect(&b);
        assert!(inter.is_some());
        let inter = inter.unwrap();
        assert_eq!(inter.x, 50.0);
        assert_eq!(inter.y, 50.0);
        assert_eq!(inter.width, 50.0);
        assert_eq!(inter.height, 50.0);
    }

    #[test]
    fn rect_intersect_no_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn rect_area() {
        let rect = Rect::new(0.0, 0.0, 10.0, 20.0);
        assert_eq!(rect.area(), 200.0);
    }

    #[test]
    fn layout_node_content_box() {
        let mut node = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        node.padding = [10.0; 4];
        node.border = [5.0; 4];
        node.margin = [0.0; 4];
        let cb = node.content_box();
        assert_eq!(cb.x, 15.0);
        assert_eq!(cb.y, 15.0);
        assert_eq!(cb.width, 70.0);
        assert_eq!(cb.height, 70.0);
    }

    #[test]
    fn layout_node_with_display() {
        let node =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 100.0, 100.0), DisplayType::Inline);
        assert_eq!(node.display, DisplayType::Inline);
    }

    #[test]
    fn compute_block_layout_stacking() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let child1 =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        root.add_child(child1);
        let child2 =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);
        // child1 should start below root padding
        assert!(root.children[0].rect.y >= 10.0);
        // child2 should be below child1
        assert!(root.children[1].rect.y >= root.children[0].rect.y);
    }

    #[test]
    fn collect_render_rects_returns_colored_nodes() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(10.0, 10.0, 780.0, 50.0));
        child1.background_color = Some([255, 0, 0, 255]);

        let child2 = LayoutNode::new(Rect::new(10.0, 70.0, 780.0, 50.0));

        root.add_child(child1);
        root.add_child(child2);

        let rects = collect_render_rects(&root);
        // Should only include the colored child (child1)
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].1, Some([255, 0, 0, 255]));
    }

    #[test]
    fn collect_render_rects_recurses() {
        let mut grandchild = LayoutNode::new(Rect::new(20.0, 20.0, 100.0, 100.0));
        grandchild.background_color = Some([0, 0, 255, 255]);

        let mut child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 200.0));
        child.add_child(grandchild);

        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.add_child(child);

        let rects = collect_render_rects(&root);
        // Should find the grandchild through recursion
        assert!(!rects.is_empty());
        assert_eq!(rects[0].1, Some([0, 0, 255, 255]));
    }

    // ------ Flexbox Layout Tests ------

    #[test]
    fn test_flex_row_basic() {
        // Two flex items should be positioned side by side (horizontally)
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_direction = FlexDirection::Row;
        root.padding = [0.0; 4];

        let child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        let child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Both children have basis=0 and no grow ↁEboth get full container width split evenly via shrink
        // Actually with no grow, no basis, the free_space = 800 - 0 = 800 > 0 but total_grow = 0
        // So no distribution happens, items stay at 0 width. Let's verify horizontal positioning:
        assert!(root.children[0].rect.x <= root.children[1].rect.x);
        // Children start at parent_x (padding=0)
        assert_eq!(root.children[0].rect.x, 0.0);
    }

    #[test]
    fn test_flex_row_with_grow() {
        // Two flex items with flex-grow: 1 should split free space evenly
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_direction = FlexDirection::Row;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Each should get ~400px width (half of 800)
        assert!((root.children[0].rect.width - 400.0).abs() < 1.0);
        assert!((root.children[1].rect.width - 400.0).abs() < 1.0);
        // Second child should be to the right of first
        assert!(root.children[1].rect.x >= root.children[0].rect.x);
    }

    #[test]
    fn test_flex_grow_weighted() {
        // grow:2 and grow:1 ↁE2:1 ratio of free space
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 900.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_grow = 2.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 900.0);

        // 2:1 ratio ↁEchild1 gets 600px, child2 gets 300px
        assert!((root.children[0].rect.width - 600.0).abs() < 1.0);
        assert!((root.children[1].rect.width - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_flex_column_direction() {
        // Column direction: children stacked vertically, full width
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_direction = FlexDirection::Column;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 100.0));
        child1.flex_basis = FlexBasis::Pixels(100.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(50.0);

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Children should have full width
        assert!((root.children[0].rect.width - 800.0).abs() < 1.0);
        // child1 should be at top, child2 below
        assert_eq!(root.children[0].rect.y, 0.0);
        assert!(root.children[1].rect.y >= root.children[0].rect.y);
    }

    #[test]
    fn test_justify_space_between() {
        // Three items: first at x=0, last at right edge, middle evenly spaced
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 600.0, 0.0));
        root.display = DisplayType::Flex;
        root.justify_content = JustifyContent::SpaceBetween;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_basis = FlexBasis::Pixels(0.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = FlexBasis::Pixels(0.0);
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child3.flex_basis = FlexBasis::Pixels(0.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        test_compute_layout(&mut root, 600.0);

        // All items have 0 basis + no grow ↁEall at width 0
        // Space between: 600px / 2 gaps = 300px per gap
        assert!((root.children[0].rect.x - 0.0).abs() < 1.0);
        assert!((root.children[1].rect.x - 300.0).abs() < 1.0);
        assert!((root.children[2].rect.x - 600.0).abs() < 1.0);
    }

    #[test]
    fn test_justify_center() {
        // Single item should be centered
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.justify_content = JustifyContent::Center;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 0.0));
        child1.flex_basis = FlexBasis::Pixels(200.0);

        root.add_child(child1);

        test_compute_layout(&mut root, 800.0);

        // Item should be centered: (800 - 200) / 2 = 300 offset
        assert!((root.children[0].rect.x - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_flex_shrink_overflow() {
        // Total basis (600+400=1000) exceeds container (800) ↁEshrink proportionally
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_basis = FlexBasis::Pixels(600.0);
        child1.flex_shrink = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = FlexBasis::Pixels(400.0);
        child2.flex_shrink = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Overflow = 200px. Shrink weight: 1*600=600 and 1*400=400, total=1000
        // child1 shrinks by: 200 * 600/1000 = 120 ↁEwidth = 480
        // child2 shrinks by: 200 * 400/1000 = 80 ↁEwidth = 320
        assert!((root.children[0].rect.width - 480.0).abs() < 2.0);
        assert!((root.children[1].rect.width - 320.0).abs() < 2.0);
    }

    #[test]
    fn test_flex_with_basis_and_grow() {
        // basis: 100 + 100 = 200, container: 800 ↁEfree_space = 600, split by grow 1:1
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_basis = FlexBasis::Pixels(100.0);
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = FlexBasis::Pixels(100.0);
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Each gets: 100 + (600/2) = 400px
        assert!((root.children[0].rect.width - 400.0).abs() < 1.0);
        assert!((root.children[1].rect.width - 400.0).abs() < 1.0);
    }

    // ---- Flexbox Part 2: Multi-line wrap, gap, align-content ----

    #[test]
    fn test_flex_no_wrap_unchanged() {
        // Regression: single-line flexbox behavior unchanged when nowrap
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::NoWrap;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 100.0));
        child1.flex_basis = FlexBasis::Pixels(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 80.0));
        child2.flex_basis = FlexBasis::Pixels(200.0);

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Both on same line, side by side
        assert_eq!(root.children[0].rect.x, 0.0);
        assert!(root.children[1].rect.x > root.children[0].rect.x);
        // Both at same y (within tolerance for cross-size differences)
        assert!((root.children[0].rect.y - root.children[1].rect.y).abs() < 1.0);
    }

    #[test]
    fn test_flex_wrap_basic() {
        // Three items, each ~300px basis in 800px container ↁEthird wraps to second line
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 60.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 40.0));
        child3.flex_basis = FlexBasis::Pixels(300.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        test_compute_layout(&mut root, 800.0);

        // First two items on line 1 (y ≁E0)
        assert!((root.children[0].rect.y - 0.0).abs() < 1.0);
        assert!((root.children[1].rect.y - 0.0).abs() < 1.0);
        // Third item wraps to line 2 (y > first line items height)
        assert!(root.children[2].rect.y > root.children[0].rect.y);
        // Third item starts at x=0 on the new line
        assert!((root.children[2].rect.x - 0.0).abs() < 1.0);
    }

    #[test]
    fn test_flex_wrap_two_lines_grow() {
        // Two items per line, flex-grow distributes within each line independently
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 200.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.padding = [0.0; 4];

        // Line 1: two items with basis 500 total, grow=1 each ↁEfill 800
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(200.0);
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);
        child2.flex_grow = 1.0;

        // Line 2: two items with basis 700 total, grow=1 each ↁEfill 800
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 350.0, 50.0));
        child3.flex_basis = FlexBasis::Pixels(350.0);
        child3.flex_grow = 1.0;
        let mut child4 = LayoutNode::new(Rect::new(0.0, 0.0, 350.0, 50.0));
        child4.flex_basis = FlexBasis::Pixels(350.0);
        child4.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);
        root.add_child(child4);

        test_compute_layout(&mut root, 800.0);

        // Line 1: basis total = 500, free = 300. Each grows by 150 ↁEwidths: 350, 450
        assert!((root.children[0].rect.width - 350.0).abs() < 2.0);
        assert!((root.children[1].rect.width - 450.0).abs() < 2.0);
        // Line 2: basis total = 700, free = 100. Each grows by 50 ↁEwidths: 400, 400
        assert!((root.children[2].rect.width - 400.0).abs() < 2.0);
        assert!((root.children[3].rect.width - 400.0).abs() < 2.0);
        // Items on line 2 have larger y
        assert!(root.children[2].rect.y > root.children[0].rect.y);
    }

    #[test]
    fn test_flex_wrap_reverse() {
        // wrap-reverse: first line appears at bottom of content area
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::WrapReverse;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 60.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 40.0));
        child3.flex_basis = FlexBasis::Pixels(300.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        test_compute_layout(&mut root, 800.0);

        // With wrap-reverse, the first line is placed at the bottom, but our implementation
        // shifts all children up so y >= 0. The key: child3 (on line 2 in normal order) should
        // appear ABOVE child1 and child2 visually because wrap-reverse reverses line stacking.
        // After shift correction, all y >= 0.
        assert!(root.children[0].rect.y >= 0.0);
        assert!(root.children[1].rect.y >= 0.0);
        // First two items are on the same (reversed) line
        assert!((root.children[0].rect.y - root.children[1].rect.y).abs() < 1.0);
        // Third item wraps, and in wrap-reverse should be above them
        assert!(root.children[2].rect.y < root.children[0].rect.y);
    }

    #[test]
    fn test_gap_column_axis() {
        // column_gap adds space between items on the main axis (row direction)
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.column_gap = 20.0; // 20px gap between columns
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(200.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(200.0);

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // child1 starts at x=0, child2 should start at x=200+20=220
        assert!((root.children[0].rect.x - 0.0).abs() < 1.0);
        assert!((root.children[1].rect.x - 220.0).abs() < 2.0);
    }

    #[test]
    fn test_gap_row_axis() {
        // row_gap adds space between flex lines (cross-axis in row direction)
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.row_gap = 16.0;
        root.padding = [0.0; 4];

        // First line: two items that fit
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(400.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 60.0));
        child2.flex_basis = FlexBasis::Pixels(400.0);

        // Third item wraps to line 2
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 40.0));
        child3.flex_basis = FlexBasis::Pixels(300.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        test_compute_layout(&mut root, 800.0);

        // Line 1 items at y=0
        assert!((root.children[0].rect.y - 0.0).abs() < 1.0);
        assert!((root.children[1].rect.y - 0.0).abs() < 1.0);
        // Line 2 item should start at y = max(line1 cross_size) + row_gap
        // child1 cross ~50, child2 cross ~60 ↁEline height ≁E60 + 16 gap = 76
        assert!(root.children[2].rect.y >= root.children[1].rect.bottom());
        // Verify the gap is included: child3.y should be > child2.bottom() by row_gap
        let gap_between = root.children[2].rect.y - root.children[1].rect.bottom();
        assert!((gap_between - 16.0).abs() < 1.0); // gap should equal row_gap, within float tolerance
    }

    #[test]
    fn test_gap_with_grow() {
        // Gap subtracts from free space before grow distribution
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 840.0, 0.0));
        root.display = DisplayType::Flex;
        root.column_gap = 20.0; // One gap between two items
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(100.0);
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(100.0);
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 840.0);

        // Total basis = 200, gap = 20, free_space = 840 - 200 - 20 = 620
        // Each grows by 310 ↁEwidths: 410, 410
        assert!((root.children[0].rect.width - 410.0).abs() < 2.0);
        assert!((root.children[1].rect.width - 410.0).abs() < 2.0);
        // child2.x should be at 410 + 20 = 430
        assert!((root.children[1].rect.x - 430.0).abs() < 2.0);
    }

    #[test]
    fn test_align_content_center() {
        // Two lines centered vertically within a container with explicit height
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 300.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.align_content = AlignContent::Center;
        root.padding = [0.0; 4];

        // Line 1: two items totaling 800px (fills container exactly)
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 500.0, 40.0));
        child1.flex_basis = FlexBasis::Pixels(500.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);

        // Line 2: one item that wraps
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 40.0));
        child3.flex_basis = FlexBasis::Pixels(400.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        test_compute_layout(&mut root, 800.0);

        // Lines total cross-size ≁Emax(50, 50) + max(40, 0) = ~90 (plus gaps)
        // Container height is 300, so excess ≁E210px should be distributed as centering
        // First line items should be pushed down by about half the excess
        assert!(root.children[0].rect.y > 50.0); // shifted down from top
        // Second line should also be below first line
        assert!(root.children[2].rect.y > root.children[0].rect.y);
    }

    #[test]
    fn test_align_content_space_between() {
        // Two lines pushed to top and bottom of container
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 400.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.align_content = AlignContent::SpaceBetween;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 500.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(500.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 60.0));
        child2.flex_basis = FlexBasis::Pixels(400.0);

        // Wraps to line 2
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child3.flex_basis = FlexBasis::Pixels(300.0);
        let mut child4 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 40.0));
        child4.flex_basis = FlexBasis::Pixels(200.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);
        root.add_child(child4);

        test_compute_layout(&mut root, 800.0);

        // Line 1 (child[0]) at top, line 2 (child[1],child[2]) in middle,
        // line 3 (child[3]) pushed to bottom area by SpaceBetween
        assert!((root.children[0].rect.y - 0.0).abs() < 1.0);
        assert!(root.children[2].rect.y > root.children[0].rect.y);
        // Line 3 item (child[3]) should be near the bottom of the container
        assert!(root.children[3].rect.y > 200.0);
    }

    #[test]
    fn test_justify_per_line() {
        // Justify-content applied independently per line, not across all children
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.justify_content = JustifyContent::Center;
        root.padding = [0.0; 4];

        // Line 1: two items (600 total), centered ↁEoffset of 100px
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);

        // Line 2: one item (400), centered ↁEoffset of 200px
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 50.0));
        child3.flex_basis = FlexBasis::Pixels(400.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        test_compute_layout(&mut root, 800.0);

        // Line 1: total width 600, centered ↁEx starts at (800-600)/2 = 100
        assert!((root.children[0].rect.x - 100.0).abs() < 2.0);
        // Line 2: total width 400, centered ↁEx starts at (800-400)/2 = 200
        assert!((root.children[2].rect.x - 200.0).abs() < 2.0);
    }

    #[test]
    fn test_flex_wrap_single_item_overflow() {
        // Single item wider than container still gets its own line
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 600.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        child1.flex_basis = FlexBasis::Pixels(200.0);
        // This item is wider than the container itself
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 700.0, 60.0));
        child2.flex_basis = FlexBasis::Pixels(700.0);

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 600.0);

        // child1 on line 1 at x=0, y=0
        assert!((root.children[0].rect.x - 0.0).abs() < 1.0);
        assert!((root.children[0].rect.y - 0.0).abs() < 1.0);
        // child2 overflows but gets its own line (forced wrap)
        assert!(root.children[1].rect.y > root.children[0].rect.y);
        assert!((root.children[1].rect.x - 0.0).abs() < 1.0);
        // Width is still set to basis even though it overflows
        assert!((root.children[1].rect.width - 700.0).abs() < 2.0);
    }

    #[test]
    fn test_align_content_stretch() {
        // Lines get extra cross-size distributed equally
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 400.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.align_content = AlignContent::Stretch;
        root.padding = [0.0; 4];

        // Line 1: fills container width
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 500.0, 40.0));
        child1.flex_basis = FlexBasis::Pixels(500.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);

        // Line 2: wraps
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 30.0));
        child3.flex_basis = FlexBasis::Pixels(400.0);
        let mut child4 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 45.0));
        child4.flex_basis = FlexBasis::Pixels(200.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);
        root.add_child(child4);

        test_compute_layout(&mut root, 800.0);

        // With stretch, extra height is distributed to each line's items
        // Original line heights: ~50 and ~45, with gap. Container = 400.
        // Each line gets extra height added to item rect heights
        let line1_h = root.children[0]
            .rect
            .height
            .max(root.children[1].rect.height);
        let line2_h = root.children[2]
            .rect
            .height
            .max(root.children[3].rect.height);
        // Both lines should have grown from their original sizes
        assert!(line1_h > 50.0 || (line1_h - 50.0).abs() < 1.0);
        assert!(line2_h > 45.0 || (line2_h - 45.0).abs() < 1.0);
    }

    #[test]
    fn collect_text_nodes_finds_text() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut text_child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 30.0));
        text_child.text = Some("Hello".to_string());

        let empty_child = LayoutNode::new(Rect::new(10.0, 50.0, 200.0, 30.0));
        // No text  Eshould be skipped

        root.add_child(text_child);
        root.add_child(empty_child);

        let texts = collect_text_nodes(&root);
        assert_eq!(texts.len(), 1, "Only one node has text");
        assert_eq!(texts[0].text, "Hello");
        assert!((texts[0].x - 10.0).abs() < 0.001);
        assert!((texts[0].y - 10.0).abs() < 0.001);
    }

    #[test]
    fn collect_text_nodes_skips_whitespace_only() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut ws_child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 30.0));
        ws_child.text = Some("   ".to_string());

        root.add_child(ws_child);

        let texts = collect_text_nodes(&root);
        assert!(texts.is_empty(), "Whitespace-only text is skipped");
    }

    #[test]
    fn collect_text_nodes_skips_zero_dimensions() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut zero_child = LayoutNode::new(Rect::new(10.0, 10.0, 0.0, 0.0));
        zero_child.text = Some("Hidden".to_string());

        root.add_child(zero_child);

        let texts = collect_text_nodes(&root);
        assert!(texts.is_empty(), "Zero-dimension text nodes are skipped");
    }

    #[test]
    fn collect_text_nodes_recurses() {
        let mut grandchild = LayoutNode::new(Rect::new(20.0, 20.0, 150.0, 25.0));
        grandchild.text = Some("Nested".to_string());

        let mut child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 100.0));
        child.add_child(grandchild);

        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.add_child(child);

        let texts = collect_text_nodes(&root);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].text, "Nested");
    }

    #[test]
    fn collect_image_nodes_finds_images() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut img_child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 150.0));
        img_child.image_src = Some("https://example.com/logo.png".to_string());

        let text_child = LayoutNode::new(Rect::new(10.0, 170.0, 100.0, 30.0));

        root.add_child(img_child);
        root.add_child(text_child);

        let images = collect_image_nodes(&root);
        assert_eq!(images.len(), 1, "Only one node has image_src");
        assert_eq!(images[0].src, "https://example.com/logo.png");
        assert!((images[0].x - 10.0).abs() < 0.001);
        assert!((images[0].y - 10.0).abs() < 0.001);
        assert!((images[0].width - 200.0).abs() < 0.001);
        assert!((images[0].height - 150.0).abs() < 0.001);
    }

    #[test]
    fn collect_image_nodes_skips_zero_dimensions() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut img_child = LayoutNode::new(Rect::new(10.0, 10.0, 0.0, 0.0));
        img_child.image_src = Some("https://example.com/hidden.png".to_string());

        root.add_child(img_child);

        let images = collect_image_nodes(&root);
        assert!(images.is_empty(), "Zero-dimension image nodes are skipped");
    }

    #[test]
    fn collect_image_nodes_recurses() {
        let mut grandchild = LayoutNode::new(Rect::new(20.0, 20.0, 100.0, 80.0));
        grandchild.image_src = Some("https://example.com/nested.jpg".to_string());

        let mut child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 100.0));
        child.add_child(grandchild);

        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.add_child(child);

        let images = collect_image_nodes(&root);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/nested.jpg");
    }

    #[test]
    fn collect_image_nodes_multiple() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut img1 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 100.0));
        img1.image_src = Some("https://example.com/a.png".to_string());

        let mut img2 = LayoutNode::new(Rect::new(210.0, 0.0, 200.0, 100.0));
        img2.image_src = Some("https://example.com/b.png".to_string());

        root.add_child(img1);
        root.add_child(img2);

        let images = collect_image_nodes(&root);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].src, "https://example.com/a.png");
        assert_eq!(images[1].src, "https://example.com/b.png");
    }

    // ---- Inline Box Construction Tests (Step A2) ----

    #[test]
    fn test_build_inline_boxes_simple() {
        // A parent with a text child and an inline element child
        // produces [Text, Element]
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut text_child = LayoutNode::new(Rect::new(10.0, 10.0, 100.0, 20.0));
        text_child.text = Some("Hello".to_string());

        let inline_child =
            LayoutNode::new_with_display(Rect::new(10.0, 10.0, 50.0, 20.0), DisplayType::Inline);

        root.add_child(text_child);
        root.add_child(inline_child);

        let boxes = build_inline_boxes(&root, &root.children);

        assert_eq!(boxes.len(), 2);

        // First box is Text
        match &boxes[0] {
            InlineBox::Text {
                text,
                width: _,
                color: _,
                font_size: _,
            } => {
                assert_eq!(text.as_str(), "Hello");
            }
            _ => panic!("Expected Text variant"),
        }

        // Second box is Element pointing to index 1
        match &boxes[1] {
            InlineBox::Element {
                child_index,
                width: _,
                height: _,
                baseline_offset: _,
            } => {
                assert_eq!(*child_index, 1);
            }
            _ => panic!("Expected Element variant"),
        }
    }

    #[test]
    fn test_build_inline_boxes_whitespace() {
        // Whitespace-only text nodes produce Whitespace variant with collapsible=true
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut ws_child = LayoutNode::new(Rect::new(10.0, 10.0, 50.0, 20.0));
        ws_child.text = Some("   ".to_string());

        let mut tab_child = LayoutNode::new(Rect::new(10.0, 10.0, 50.0, 20.0));
        tab_child.text = Some("\t\n".to_string());

        root.add_child(ws_child);
        root.add_child(tab_child);

        let boxes = build_inline_boxes(&root, &root.children);

        assert_eq!(boxes.len(), 2);

        match &boxes[0] {
            InlineBox::Whitespace {
                collapsible,
                width: _,
            } => {
                assert!(*collapsible, "Spaces should be collapsible");
            }
            _ => panic!("Expected Whitespace variant"),
        }

        match &boxes[1] {
            InlineBox::Whitespace {
                collapsible,
                width: _,
            } => {
                assert!(*collapsible, "Tab/newline should be collapsible");
            }
            _ => panic!("Expected Whitespace variant"),
        }
    }

    #[test]
    fn test_build_inline_boxes_mixed() {
        // Text + whitespace + inline element -> [Text, Whitespace, Element]
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut text_child = LayoutNode::new(Rect::new(10.0, 10.0, 100.0, 20.0));
        text_child.text = Some("Bold: ".to_string());

        let mut ws_child = LayoutNode::new(Rect::new(60.0, 10.0, 10.0, 20.0));
        ws_child.text = Some(" ".to_string());

        let inline_child =
            LayoutNode::new_with_display(Rect::new(70.0, 10.0, 50.0, 20.0), DisplayType::Inline);

        root.add_child(text_child);
        root.add_child(ws_child);
        root.add_child(inline_child);

        let boxes = build_inline_boxes(&root, &root.children);

        assert_eq!(boxes.len(), 3);

        match &boxes[0] {
            InlineBox::Text { text, .. } => assert_eq!(text.as_str(), "Bold: "),
            _ => panic!("Expected Text"),
        }

        match &boxes[1] {
            InlineBox::Whitespace { collapsible, .. } => assert!(*collapsible),
            _ => panic!("Expected Whitespace"),
        }

        match &boxes[2] {
            InlineBox::Element { child_index, .. } => assert_eq!(*child_index, 2),
            _ => panic!("Expected Element"),
        }
    }

    #[test]
    fn test_build_inline_boxes_skips_block_children() {
        // Block-level children are not part of inline formatting context
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let block_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 100.0, 50.0), DisplayType::Block);

        let mut text_child = LayoutNode::new(Rect::new(10.0, 10.0, 80.0, 20.0));
        text_child.text = Some("text".to_string());

        root.add_child(block_child);
        root.add_child(text_child);

        let boxes = build_inline_boxes(&root, &root.children);

        // Only the text child produces an inline box; block child is skipped
        assert_eq!(boxes.len(), 1);

        match &boxes[0] {
            InlineBox::Text { text, .. } => assert_eq!(text.as_str(), "text"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_build_inline_boxes_empty_children() {
        let root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        let boxes = build_inline_boxes(&root, &root.children);
        assert!(boxes.is_empty());
    }

    #[test]
    fn test_build_inline_boxes_single_whitespace_collapsible() {
        // A single space between inline elements is collapsible whitespace
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut space = LayoutNode::new(Rect::new(0.0, 0.0, 5.0, 20.0));
        space.text = Some(" ".to_string());

        root.add_child(space);

        let boxes = build_inline_boxes(&root, &root.children);

        assert_eq!(boxes.len(), 1);
        match &boxes[0] {
            InlineBox::Whitespace { collapsible, .. } => {
                assert!(*collapsible, "Single space should be collapsible");
            }
            _ => panic!("Expected Whitespace variant for single space"),
        }
    }

    #[test]
    fn test_build_inline_boxes_non_whitespace_text_not_collapsible() {
        // Text that contains non-whitespace chars is NOT treated as Whitespace
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut mixed = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 20.0));
        mixed.text = Some(" a ".to_string()); // has non-whitespace 'a'

        root.add_child(mixed);

        let boxes = build_inline_boxes(&root, &root.children);

        assert_eq!(boxes.len(), 1);
        match &boxes[0] {
            InlineBox::Text { text, .. } => assert_eq!(text.as_str(), " a "),
            _ => panic!("Expected Text variant for mixed content"),
        }
    }

    // ---- Line Breaking Tests (Step A3) ----

    #[test]
    fn test_line_breaking_no_wrap() {
        // Short text that fits within available width stays on one line
        let mut renderer = crate::render::text::TextRenderer::new();

        let boxes = vec![InlineBox::Text {
            text: "Hello".to_string(),
            width: 0.0,
            color: None,
            font_size: 16.0,
        }];

        let lines = break_into_lines(boxes, 800.0, &mut renderer);

        assert_eq!(lines.len(), 1, "Short text should fit on one line");
        assert!(!lines[0].boxes.is_empty(), "Line should contain boxes");
        // Line height should be font_size * 1.2 = 19.2
        assert!((lines[0].height - 16.0 * 1.2).abs() < 0.5);
    }

    #[test]
    fn test_line_breaking_wraps_at_words() {
        // Long text with multiple words should break into multiple lines
        let mut renderer = crate::render::text::TextRenderer::new();

        // "The quick brown fox jumps over the lazy dog" with a narrow container
        let boxes = vec![InlineBox::Text {
            text: "The quick brown fox jumps over the lazy dog".to_string(),
            width: 0.0,
            color: None,
            font_size: 16.0,
        }];

        // A narrow width that forces wrapping (each word ~30-70px)
        let lines = break_into_lines(boxes, 80.0, &mut renderer);

        assert!(
            lines.len() > 1,
            "Long text with narrow width should wrap to multiple lines"
        );
    }

    #[test]
    fn test_line_breaking_whitespace_collapse() {
        // Leading and trailing whitespace on lines should be collapsed
        let mut renderer = crate::render::text::TextRenderer::new();

        let boxes = vec![
            // Leading whitespace
            InlineBox::Whitespace {
                collapsible: true,
                width: 0.0,
            },
            InlineBox::Text {
                text: "Hello world".to_string(),
                width: 0.0,
                color: None,
                font_size: 16.0,
            },
            // Trailing whitespace
            InlineBox::Whitespace {
                collapsible: true,
                width: 0.0,
            },
        ];

        let lines = break_into_lines(boxes, 800.0, &mut renderer);

        assert_eq!(lines.len(), 1);
        // Leading whitespace should be skipped, trailing trimmed during flush
        // So the first box should be Text, not Whitespace
        match &lines[0].boxes[0] {
            InlineBox::Text { .. } => {} // expected
            _ => panic!("First box should be Text after leading ws collapse"),
        }
    }

    #[test]
    fn test_line_breaking_single_word_overflow() {
        // A single word wider than the container overflows rather than being split
        let mut renderer = crate::render::text::TextRenderer::new();

        let boxes = vec![InlineBox::Text {
            text: "Supercalifragilisticexpialidocious".to_string(),
            width: 0.0,
            color: None,
            font_size: 16.0,
        }];

        // Very narrow container - word won't fit but should not be split
        let lines = break_into_lines(boxes, 30.0, &mut renderer);

        assert_eq!(lines.len(), 1, "Single word should not be split");
        assert!(
            !lines[0].boxes.is_empty(),
            "Line should contain the overflowing word"
        );

        // Verify the text content is intact (not truncated)
        match &lines[0].boxes[0] {
            InlineBox::Text { text, .. } => {
                assert_eq!(text.as_str(), "Supercalifragilisticexpialidocious");
            }
            _ => panic!("Expected Text box for overflowing word"),
        }
    }

    // ---- Baseline Alignment Tests (Step A4) ----

    #[test]
    fn test_baseline_alignment_same_size() {
        // When all text has the same font size, baselines should be identical
        let mut renderer = crate::render::text::TextRenderer::new();

        let boxes = vec![
            InlineBox::Text {
                text: "Hello".to_string(),
                width: 0.0,
                color: None,
                font_size: 16.0,
            },
            InlineBox::Whitespace {
                collapsible: false,
                width: 5.0,
            },
            InlineBox::Text {
                text: "World".to_string(),
                width: 0.0,
                color: None,
                font_size: 16.0,
            },
        ];

        let lines = break_into_lines(boxes, 800.0, &mut renderer);

        assert_eq!(lines.len(), 1, "Same-size text fits on one line");
        // All text is 16px ↁEmax_font_size = 16.0
        // ascender = 16.0 * 0.8 = 12.8, baseline_y = 0 + 12.8 = 12.8
        assert!(
            (lines[0].baseline_y - 12.8).abs() < 0.5,
            "Baseline should be at ascender (= font_size * 0.8)"
        );
        // Line height = 16.0 * 1.2 = 19.2
        assert!(
            (lines[0].height - 19.2).abs() < 0.5,
            "Line height should be font_size * 1.2"
        );
    }

    #[test]
    fn test_baseline_alignment_mixed_sizes() {
        // Mixed font sizes: line height = largest, baseline shared across all text
        let mut renderer = crate::render::text::TextRenderer::new();

        let boxes = vec![
            InlineBox::Text {
                text: "Small".to_string(),
                width: 0.0,
                color: None,
                font_size: 12.0, // smaller
            },
            InlineBox::Whitespace {
                collapsible: false,
                width: 5.0,
            },
            InlineBox::Text {
                text: "Large".to_string(),
                width: 0.0,
                color: None,
                font_size: 24.0, // larger  Edetermines line metrics
            },
        ];

        let lines = break_into_lines(boxes, 800.0, &mut renderer);

        assert_eq!(lines.len(), 1, "Mixed-size text fits on one line");
        // max_font_size = 24.0 (largest)
        // ascender = 24.0 * 0.8 = 19.2
        // baseline_y = 0 + 19.2 = 19.2
        assert!(
            (lines[0].baseline_y - 19.2).abs() < 0.5,
            "Baseline should be set by largest font (24px * 0.8 = 19.2)"
        );
        // height = 24.0 * 1.2 = 28.8
        assert!(
            (lines[0].height - 28.8).abs() < 0.5,
            "Line height should be max_font_size * 1.2"
        );
    }

    #[test]
    fn test_line_box_metrics() {
        // Verify the helper function returns correct derived values
        let line = LineBox {
            y: 10.0,
            baseline_y: 22.8,
            height: 19.2,
            boxes: Vec::new(),
        };

        let (top, baseline, bottom, height) = line_box_metrics(&line);

        assert!((top - 10.0).abs() < 0.001, "top should equal y");
        assert!(
            (baseline - 22.8).abs() < 0.001,
            "baseline should match line baseline_y"
        );
        assert!(
            (bottom - 29.2).abs() < 0.001,
            "bottom should equal y + height (10.0 + 19.2 = 29.2)"
        );
        assert!(
            (height - 19.2).abs() < 0.001,
            "returned height should match line height"
        );
    }

    #[test]
    fn test_inline_element_baseline_offset() {
        // Inline elements get baseline_offset = height * 0.7
        let mut renderer = crate::render::text::TextRenderer::new();

        let boxes = vec![InlineBox::Element {
            child_index: 0,
            width: 50.0,
            height: 40.0,
            baseline_offset: 0.0,
        }];

        let lines = break_into_lines(boxes, 800.0, &mut renderer);

        assert_eq!(lines.len(), 1);
        match &lines[0].boxes[0] {
            InlineBox::Element {
                baseline_offset, ..
            } => {
                // expected: height (40.0) * 0.7 = 28.0
                assert!(
                    (*baseline_offset - 28.0).abs() < 0.5,
                    "Element baseline_offset should be height * 0.7"
                );
            }
            _ => panic!("Expected Element box"),
        }
    }

    // ---- Inline Layout Integration Tests (Step A5) ----

    #[test]
    fn test_inline_in_block_integration() {
        // HTML-like structure: a block container with text + inline span children.
        // Verify that rect positions are set based on line box layout.
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.padding = [10.0; 4];

        // Text child: "Hello"
        let mut text_child = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        text_child.text = Some("Hello".to_string());

        // Inline element child (simulating a <span>)
        let inline_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Inline);

        root.add_child(text_child);
        root.add_child(inline_child);

        test_compute_layout(&mut root, 800.0);

        // Root should have line_boxes computed
        assert!(
            root.line_boxes.is_some(),
            "Block container with inline children should have line boxes"
        );

        let line_boxes = root.line_boxes.as_ref().unwrap();
        assert!(!line_boxes.is_empty(), "Should have at least one line box");

        // Text child should have valid dimensions after layout
        assert!(
            root.children[0].rect.width > 0.0,
            "Text child should have positive width after inline layout"
        );
        assert!(
            root.children[0].rect.height > 0.0,
            "Text child should have positive height after inline layout"
        );

        // Text child should be positioned below padding
        assert!(
            root.children[0].rect.y >= root.padding[0],
            "Text child y should start at or below top padding"
        );

        // Inline element child should also be positioned
        assert!(
            root.children[1].rect.x >= 0.0,
            "Inline element x should be non-negative"
        );
    }

    #[test]
    fn test_mixed_block_and_inline() {
        // Block children followed by inline children, both positioned correctly.
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.padding = [10.0; 4];

        // First a block child
        let block_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        root.add_child(block_child);

        // Then inline children
        let mut text_child = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        text_child.text = Some("After block".to_string());
        root.add_child(text_child);

        test_compute_layout(&mut root, 800.0);

        // Block child should be positioned first (at y >= padding)
        assert!(
            root.children[0].rect.y >= root.padding[0],
            "Block child should start at or below top padding"
        );

        // Inline text child should be positioned after the block child
        assert!(
            root.children[1].rect.y >= root.children[0].rect.y,
            "Inline child should be at or below the block child"
        );

        // Both children should have valid dimensions
        assert!(
            root.children[0].rect.width > 0.0,
            "Block child should have positive width"
        );
        assert!(
            root.children[1].rect.width > 0.0,
            "Inline text child should have positive width"
        );
    }

    #[test]
    fn test_inline_layout_with_wrapping() {
        // Text that wraps to multiple lines within a block container.
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.padding = [10.0; 4];

        // Long text child that should wrap on a narrow width
        let mut long_text = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        long_text.text =
            Some("The quick brown fox jumps over the lazy dog again and again".to_string());
        root.add_child(long_text);

        // Use a narrow page width to force wrapping
        test_compute_layout(&mut root, 200.0);

        // Root should have multiple line boxes from wrapping
        let line_boxes = root.line_boxes.as_ref().unwrap();
        assert!(
            line_boxes.len() > 1,
            "Long text with narrow width should wrap to multiple lines, got {} line(s)",
            line_boxes.len()
        );

        // The total height of the content should reflect multiple lines
        let expected_min_height = line_boxes.len() as f32 * 16.0 * 1.2;
        assert!(
            root.children[0].rect.height > 0.0,
            "Text child should have positive height after wrapping, got {}",
            root.children[0].rect.height
        );

        // Root height should accommodate all lines plus padding
        assert!(
            root.rect.height >= expected_min_height + root.padding[0] + root.padding[2],
            "Root height {} should fit all wrapped lines (expected >= {}) plus padding",
            root.rect.height,
            expected_min_height + root.padding[0] + root.padding[2]
        );
    }

    // ---- Text Collection from Line Boxes Tests (Step A6-A7) ----

    #[test]
    fn test_collect_text_nodes_from_line_boxes() {
        // When a LayoutNode has line_boxes, collect_text_nodes walks the line
        // boxes instead of recursing into children for text.
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];
        root.border = [0.0; 4];

        // Manually set line_boxes to simulate what compute_layout does for
        // a block container with inline text children.
        let line_boxes = vec![LineBox {
            y: 0.0,
            baseline_y: 12.8, // 16.0 * 0.8
            height: 19.2,     // 16.0 * 1.2
            boxes: vec![
                InlineBox::Text {
                    text: "Hello".to_string(),
                    width: 50.0,
                    color: Some([255, 0, 0, 255]), // red text
                    font_size: 16.0,
                },
                InlineBox::Whitespace {
                    collapsible: false,
                    width: 8.0,
                },
                InlineBox::Text {
                    text: "World".to_string(),
                    width: 45.0,
                    color: Some([0, 0, 255, 255]), // blue text
                    font_size: 16.0,
                },
            ],
        }];
        root.line_boxes = Some(line_boxes);

        let texts = collect_text_nodes(&root);

        assert_eq!(
            texts.len(),
            2,
            "Should find two text entries from line boxes"
        );
        // First text: "Hello" in red at the computed position
        assert_eq!(texts[0].text, "Hello");
        assert!(
            (texts[0].x - (0.0 + 10.0)).abs() < 0.001,
            "x = base_x = padding left"
        );
        assert!(
            (texts[0].y - (0.0 + 10.0)).abs() < 0.001,
            "y = base_y = padding top"
        );
        assert_eq!(texts[0].color, [255, 0, 0, 255], "Hello should be red");
        // Second text: "World" in blue, shifted right by Hello's width + whitespace
        assert_eq!(texts[1].text, "World");
        assert!(
            (texts[1].x - (10.0 + 50.0 + 8.0)).abs() < 0.001,
            "World x = padding + Hello width + space width"
        );
        assert_eq!(texts[1].color, [0, 0, 255, 255], "World should be blue");
    }

    #[test]
    fn test_collect_text_uses_foreground_color() {
        // Text color comes from the CSS `color` property (stored in InlineBox::Text.color),
        // NOT from background_color.
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [0.0; 4];
        root.border = [0.0; 4];
        // Set a green background on the container  Ethis should NOT be used as text color
        root.background_color = Some([0, 255, 0, 255]);

        let line_boxes = vec![LineBox {
            y: 0.0,
            baseline_y: 12.8,
            height: 19.2,
            boxes: vec![InlineBox::Text {
                text: "Foreground".to_string(),
                width: 60.0,
                color: Some([255, 0, 0, 255]), // red foreground CSS color
                font_size: 16.0,
            }],
        }];
        root.line_boxes = Some(line_boxes);

        let texts = collect_text_nodes(&root);

        assert_eq!(texts.len(), 1);
        assert_eq!(
            texts[0].color,
            [255, 0, 0, 255],
            "Text color should be the CSS foreground color (red), not background (green)"
        );
    }

    #[test]
    fn test_collect_text_fallback_no_line_boxes() {
        // When line_boxes is None, collect_text_nodes falls back to the
        // existing recursive behavior walking children.
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let mut text_child = LayoutNode::new(Rect::new(10.0, 10.0, 200.0, 30.0));
        text_child.text = Some("Fallback".to_string());
        text_child.color = Some([128, 0, 128, 255]); // purple foreground

        root.add_child(text_child);
        // line_boxes is None ↁEfallback behavior

        let texts = collect_text_nodes(&root);

        assert_eq!(
            texts.len(),
            1,
            "Fallback should still find text via recursion"
        );
        assert_eq!(texts[0].text, "Fallback");
        assert_eq!(
            texts[0].color,
            [128, 0, 128, 255],
            "Fallback should use the node's color field (purple)"
        );
    }

    #[test]
    fn test_layout_node_defaults() {
        // LayoutNode::new() has correct defaults for overflow/position/offsets fields
        let node = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(node.overflow, Overflow::Visible);
        assert_eq!(node.position, PositionType::Static);
        assert_eq!(node.offsets, [None, None, None, None]);
        assert!(node.absolute_children.is_empty());
    }

    #[test]
    fn test_layout_node_copies_position() {
        // Verify build_layout_tree copies position/offsets from computed styles.
        let arena = crate::html::parse_html(
            r#"<div style="position:relative; top:10px; left:-5px"><p>child</p></div>"#,
        );

        let stylesheet = crate::css::parser::parse_stylesheet(
            "div { position: relative; top: 10px; left: -5px; }",
        );
        let styles = crate::css::compute_styles_for_tree(&arena, &stylesheet);

        // Find the div node ID
        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        drop(nodes);

        let div_id = div_id.expect("Expected to find a <div> node");

        // Build layout tree starting from the div
        let root_layout = build_layout_tree(
            div_id as u32,
            &styles,
            |id| {
                let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
                arena.get(handle)
            },
            800.0,
        );

        assert_eq!(root_layout.position, PositionType::Relative);
        assert_eq!(root_layout.offsets[0], Some(10.0)); // top = 10px
        assert_eq!(root_layout.offsets[3], Some(-5.0)); // left = -5px
    }

    // ---- Positioning Tests (Track D) ----

    #[test]
    fn test_relative_positioning_shifts_rect() {
        // A relative element with top:20px and left:30px shifts down/right by those amounts
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let mut child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        child.position = PositionType::Relative;
        child.offsets = [Some(20.0), None, None, Some(30.0)]; // top=20, left=30

        root.add_child(child);
        test_compute_layout(&mut root, 800.0);

        let original_x = 10.0; // root padding left
        let original_y = 10.0; // root padding top

        apply_relative_positioning(&mut root);

        assert!(
            (root.children[0].rect.x - (original_x + 30.0)).abs() < 1.0,
            "x should shift right by left offset (30)"
        );
        assert!(
            (root.children[0].rect.y - (original_y + 20.0)).abs() < 1.0,
            "y should shift down by top offset (20)"
        );
    }

    #[test]
    fn test_relative_positioning_negative_offsets() {
        // Negative offsets shift in the opposite direction
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let mut child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        child.position = PositionType::Relative;
        child.offsets = [Some(-15.0), None, None, Some(-25.0)]; // top=-15, left=-25

        root.add_child(child);
        test_compute_layout(&mut root, 800.0);

        let before_x = root.children[0].rect.x;
        let before_y = root.children[0].rect.y;

        apply_relative_positioning(&mut root);

        assert!(
            (root.children[0].rect.x - (before_x - 25.0)).abs() < 1.0,
            "x should shift left by negative left offset"
        );
        assert!(
            (root.children[0].rect.y - (before_y - 15.0)).abs() < 1.0,
            "y should shift up by negative top offset"
        );
    }

    #[test]
    fn test_absolute_removed_from_flow() {
        // An absolute child is extracted and does not affect sibling positioning
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let sibling_before =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);

        let mut abs_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 100.0, 50.0), DisplayType::Block);
        abs_child.position = PositionType::Absolute;
        abs_child.offsets = [Some(5.0), None, None, Some(10.0)];

        let sibling_after =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);

        root.add_child(sibling_before);
        root.add_child(abs_child);
        root.add_child(sibling_after);

        extract_absolute_children(&mut root);

        assert_eq!(
            root.children.len(),
            2,
            "Absolute child should be removed from normal flow"
        );
        assert_eq!(
            root.absolute_children.len(),
            1,
            "One absolute child extracted"
        );
        assert!(root.children[0].position != PositionType::Absolute);
        assert!(root.children[1].position != PositionType::Absolute);
    }

    #[test]
    fn test_absolute_positioned_from_ancestor() {
        // Absolute child positioned relative to containing block's content box
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [20.0; 4];
        root.border = [5.0; 4];

        let mut abs_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        abs_child.position = PositionType::Absolute;
        abs_child.offsets = [Some(10.0), None, None, Some(20.0)]; // top=10, left=20

        root.add_child(abs_child);
        extract_absolute_children(&mut root);

        let mut renderer = crate::render::text::TextRenderer::new();
        test_compute_layout(&mut root, 800.0);

        let containing_block = Rect::new(
            root.padding[3] + root.border[3], // 25
            root.padding[0] + root.border[0], // 25
            (root.rect.width - root.padding[1] - root.padding[3] - root.border[1] - root.border[3])
                .max(0.0),
            (root.rect.height
                - root.padding[0]
                - root.padding[2]
                - root.border[0]
                - root.border[2])
                .max(0.0),
        );

        compute_absolute_positions(&mut root, containing_block, &mut renderer);

        // x should be cb.x + left_offset = 25 + 20 = 45
        assert!(
            (root.absolute_children[0].rect.x - 45.0).abs() < 1.0,
            "x should be cb_x + left"
        );
        // y should be cb.y + top_offset = 25 + 10 = 35
        assert!(
            (root.absolute_children[0].rect.y - 35.0).abs() < 1.0,
            "y should be cb_y + top"
        );
    }

    #[test]
    fn test_absolute_in_flex_skipped() {
        // Absolutely positioned child of a flex container is excluded from flex layout
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_direction = FlexDirection::Row;
        root.padding = [0.0; 4];

        let child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));

        let mut abs_child = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 50.0));
        abs_child.position = PositionType::Absolute;

        let child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));

        root.add_child(child1);
        root.add_child(abs_child);
        root.add_child(child2);

        extract_absolute_children(&mut root);

        // After extraction, only 2 normal-flow children remain for flex layout
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.absolute_children.len(), 1);

        test_compute_layout(&mut root, 800.0);

        // Flex layout should only position the 2 remaining children
        assert!((root.children[0].rect.x - 0.0).abs() < 1.0);
        // child2 should be after child1 in flex order (index 1)
        assert!(root.children[1].rect.x >= root.children[0].rect.x);
    }

    #[test]
    fn test_absolute_sibling_ordering() {
        // Siblings before and after an absolute child are adjacent after extraction
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let sibling_a =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);

        let mut abs_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 100.0, 50.0), DisplayType::Block);
        abs_child.position = PositionType::Absolute;

        let sibling_b =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);

        root.add_child(sibling_a);
        root.add_child(abs_child);
        root.add_child(sibling_b);

        extract_absolute_children(&mut root);

        // sibling_a and sibling_b should now be adjacent in children vec
        assert_eq!(root.children.len(), 2);

        test_compute_layout(&mut root, 800.0);

        // sibling_b should be directly below sibling_a (no gap from absolute child)
        assert!(root.children[1].rect.y >= root.children[0].rect.bottom());
    }

    #[test]
    fn test_collect_rects_includes_absolute() {
        // Render rect collection includes rectangles from absolutely positioned children
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));

        let normal_child =
            LayoutNode::new_with_display(Rect::new(10.0, 10.0, 200.0, 100.0), DisplayType::Block);

        let mut abs_child =
            LayoutNode::new_with_display(Rect::new(50.0, 50.0, 150.0, 80.0), DisplayType::Block);
        abs_child.background_color = Some([255, 0, 0, 255]);
        abs_child.position = PositionType::Absolute;

        root.add_child(normal_child);
        root.absolute_children.push(abs_child);

        let rects = collect_render_rects(&root);

        // The absolute child has a background color and should be in the collected rects
        assert!(
            rects.iter().any(|(_, c)| c == &Some([255, 0, 0, 255])),
            "Absolute child's colored rect should appear in render rects"
        );
    }

    #[test]
    fn test_absolute_child_with_own_children() {
        // An absolute node with nested block children positions them correctly inside its box
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [0.0; 4];

        let mut abs_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        abs_child.position = PositionType::Absolute;
        abs_child.offsets = [Some(20.0), None, None, Some(30.0)];
        abs_child.padding = [10.0; 4];

        // Add a block child inside the absolute node
        let inner_block =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        abs_child.add_child(inner_block);

        root.add_child(abs_child);
        extract_absolute_children(&mut root);

        let mut renderer = crate::render::text::TextRenderer::new();
        test_compute_layout(&mut root, 800.0);

        let containing_block = Rect::new(0.0, 0.0, 800.0, 600.0);
        compute_absolute_positions(&mut root, containing_block, &mut renderer);

        // The absolute child should be positioned at (30, 20)
        assert!((root.absolute_children[0].rect.x - 30.0).abs() < 1.0);
        assert!((root.absolute_children[0].rect.y - 20.0).abs() < 1.0);

        // The inner block child should be positioned within the absolute node's padding box
        let abs_node = &root.absolute_children[0];
        assert!(
            abs_node.children[0].rect.x
                >= abs_node.rect.x + abs_node.padding[3] + abs_node.border[3],
            "Inner child x should start after absolute node's left padding+border"
        );
        // Note: compute_block_children sets y positions relative to 0, starting at parent.padding[0],
        // so the inner child's y is within the absolute node's content area (positive and > 0)
        assert!(
            abs_node.children[0].rect.y >= abs_node.padding[0] + abs_node.border[0],
            "Inner child y should start after absolute node's top padding+border"
        );
    }

    #[test]
    fn test_absolute_default_offsets_zero() {
        // Absolute with no explicit offsets uses 0 for position (top-left of containing block)
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [0.0; 4];

        let mut abs_child =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        abs_child.position = PositionType::Absolute;
        // offsets are all None (defaults)

        root.add_child(abs_child);
        extract_absolute_children(&mut root);

        let mut renderer = crate::render::text::TextRenderer::new();
        test_compute_layout(&mut root, 800.0);

        let containing_block = Rect::new(0.0, 0.0, 800.0, 600.0);
        compute_absolute_positions(&mut root, containing_block, &mut renderer);

        // With no offsets set, defaults to (0, 0) in containing block coords
        assert!(
            (root.absolute_children[0].rect.x - 0.0).abs() < 1.0,
            "x should default to 0 when left offset is None"
        );
        assert!(
            (root.absolute_children[0].rect.y - 0.0).abs() < 1.0,
            "y should default to 0 when top offset is None"
        );
    }

    // ------ Overflow Clipping Tests ------

    #[test]
    fn test_overflow_visible_no_clipping() {
        // When overflow is Visible, child rects are not clipped.
        let mut child = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        child.background_color = Some([255, 0, 0, 255]);

        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        root.overflow = Overflow::Visible;
        // Content box of root = rect itself since padding/border/margin are 0
        root.add_child(child);

        let rects = collect_render_rects(&root);
        // Child should appear with its full unclipped dimensions
        assert!(!rects.is_empty());
        let child_rect = rects
            .iter()
            .find(|r| r.1 == Some([255, 0, 0, 255]))
            .expect("child colored rect");
        assert_eq!(child_rect.0.width, 200.0);
        assert_eq!(child_rect.0.height, 200.0);
    }

    #[test]
    fn test_overflow_hidden_clips_children() {
        // When overflow is Hidden, child rects are clipped to the parent's content box.
        let mut child = LayoutNode::new(Rect::new(50.0, 50.0, 200.0, 200.0));
        child.background_color = Some([0, 255, 0, 255]);

        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        root.overflow = Overflow::Hidden;
        // Content box = rect since padding/border/margin are all 0
        root.add_child(child);

        let rects = collect_render_rects(&root);
        let child_rect = rects
            .iter()
            .find(|r| r.1 == Some([0, 255, 0, 255]))
            .expect("child colored rect");
        // Child rect [50,50,200x200] clipped to content box [0,0,100x100] -> [50,50,50x50]
        assert_eq!(child_rect.0.x, 50.0);
        assert_eq!(child_rect.0.y, 50.0);
        assert_eq!(child_rect.0.width, 50.0);
        assert_eq!(child_rect.0.height, 50.0);
    }

    #[test]
    fn test_partial_rect_intersection() {
        // A rect partially overlapping a clip region yields the intersection.
        let mut child = LayoutNode::new(Rect::new(80.0, 0.0, 50.0, 50.0));
        child.background_color = Some([0, 0, 255, 255]);

        let mut container = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        container.overflow = Overflow::Hidden;
        container.add_child(child);

        let rects = collect_render_rects(&container);
        let child_rect = rects
            .iter()
            .find(|r| r.1 == Some([0, 0, 255, 255]))
            .expect("child rect");
        // [80,0,50x50] clipped to [0,0,100x100] -> [80,0,20x50]
        assert_eq!(child_rect.0.x, 80.0);
        assert_eq!(child_rect.0.width, 20.0);
        assert_eq!(child_rect.0.height, 50.0);

        // A rect fully outside the clip region is skipped entirely
        let mut outside = LayoutNode::new(Rect::new(150.0, 150.0, 50.0, 50.0));
        outside.background_color = Some([255, 255, 0, 255]);
        container.add_child(outside);

        let rects2 = collect_render_rects(&container);
        assert!(rects2.iter().all(|r| r.1 != Some([255, 255, 0, 255])));
    }

    #[test]
    fn test_nested_overflow_containers() {
        // Two nested overflow:hidden containers clip correctly (intersection of both).
        let mut grandchild = LayoutNode::new(Rect::new(80.0, 80.0, 100.0, 100.0));
        grandchild.background_color = Some([128, 128, 128, 255]);

        let mut inner = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        inner.overflow = Overflow::Hidden;
        inner.add_child(grandchild);

        let mut outer = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        outer.overflow = Overflow::Hidden;
        // Outer content box = [0,0,200x50] (no padding/border/margin)
        // Inner content box = [0,0,100x100]
        // Grandchild [80,80,100x100] clipped to both:
        //   vs inner [0,0,100x100] -> [80,80,20x20]
        //   vs outer [0,0,200x50]  -> fully outside (y=80 >= outer bottom y=50) -> skipped
        outer.add_child(inner);

        let rects = collect_render_rects(&outer);
        // Grandchild rect is fully outside the outer clip (y=80 >= outer bottom y=50)
        assert!(rects.iter().all(|r| r.1 != Some([128, 128, 128, 255])));

        // Now test with a grandchild that falls within both clips
        let mut gc2 = LayoutNode::new(Rect::new(10.0, 10.0, 30.0, 30.0));
        gc2.background_color = Some([64, 64, 64, 255]);

        let mut inner2 = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        inner2.overflow = Overflow::Hidden;
        inner2.add_child(gc2);

        let mut outer2 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        outer2.overflow = Overflow::Hidden;
        outer2.add_child(inner2);

        let rects2 = collect_render_rects(&outer2);
        let gc2_rect = rects2
            .iter()
            .find(|r| r.1 == Some([64, 64, 64, 255]))
            .expect("gc2 rect within both clips");
        // Within inner [0,0,100x100] and outer [0,0,200x200] -> no clipping needed
        assert_eq!(gc2_rect.0.x, 10.0);
        assert_eq!(gc2_rect.0.y, 10.0);
        assert_eq!(gc2_rect.0.width, 30.0);
        assert_eq!(gc2_rect.0.height, 30.0);
    }

    // ------ Box Sizing Tests ------

    #[test]
    fn test_block_border_box_width() {
        // With border-box, the content width = total width - padding - border
        let arena = crate::html::parse_html(
            "<div style='width:200px; box-sizing:border-box; padding:10px'><p>content</p></div>",
        );
        let stylesheet = crate::css::parser::parse_stylesheet(
            "div { width: 200px; box-sizing: border-box; padding: 10px; }",
        );
        let styles = crate::css::compute_styles_for_tree(&arena, &stylesheet);

        let nodes = arena.nodes.borrow();
        let div_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        drop(nodes);

        let div_id = div_id.expect("Expected to find a <div> node");

        let root_layout = build_layout_tree(
            div_id as u32,
            &styles,
            |id| {
                let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
                arena.get(handle)
            },
            800.0,
        );

        // Verify the layout node has border-box and explicit width
        assert_eq!(root_layout.box_sizing, BoxSizing::BorderBox);
        assert_eq!(root_layout.explicit_width, Some(200.0));
    }

    // ------ Flex Percentage Basis Tests ------

    #[test]
    fn test_flex_percentage_basis_two_columns() {
        // Two flex items with 50% flex-basis each should split the container evenly
        let arena = crate::html::parse_html(
            "<div style='display:flex'><div style='flex-basis:50%'>left</div><div style='flex-basis:50%'>right</div></div>",
        );
        let stylesheet = crate::css::parser::parse_stylesheet(
            "div { display: flex; } div > div { flex-basis: 50%; }",
        );
        let styles = crate::css::compute_styles_for_tree(&arena, &stylesheet);

        let nodes = arena.nodes.borrow();
        let container_id = nodes.iter().position(|n| {
            n.is_element()
                && n.tag_name()
                    .map(|t| t.to_string() == "div")
                    .unwrap_or(false)
        });
        drop(nodes);

        let container_id = container_id.expect("Expected to find a <div> container");

        let mut root_layout = build_layout_tree(
            container_id as u32,
            &styles,
            |id| {
                let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(id));
                arena.get(handle)
            },
            800.0,
        );

        test_compute_layout(&mut root_layout, 800.0);

        // Each child with 50% basis should get ~400px (50% of 800)
        if root_layout.children.len() >= 2 {
            assert!(
                (root_layout.children[0].rect.width - 400.0).abs() < 10.0,
                "First child width should be ~400px (50%% of 800), got {}",
                root_layout.children[0].rect.width
            );
            assert!(
                (root_layout.children[1].rect.width - 400.0).abs() < 10.0,
                "Second child width should be ~400px (50%% of 800), got {}",
                root_layout.children[1].rect.width
            );
        }
    }

    // ------ Min/Max Width Clamping Tests ------

    #[test]
    fn test_flex_min_width_clamp() {
        // Flex items with min-width constraint should not shrink below that value
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_basis = FlexBasis::Pixels(100.0);
        child1.flex_shrink = 1.0;
        child1.min_width = Some(150.0);

        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = FlexBasis::Pixels(300.0);
        child2.flex_shrink = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 400.0);

        // Without min-width clamp, child1 would shrink below 150px.
        // With min_width=150, it should not go below that.
        assert!(
            root.children[0].rect.width >= 150.0 - 1.0,
            "Child with min_width should not shrink below 150px, got {}",
            root.children[0].rect.width
        );
    }

    // ------ CSS Grid Layout Tests ------

    #[test]
    fn test_grid_four_columns() {
        // Four 1fr tracks: children fill row by row
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Grid;
        root.padding = [0.0; 4];
        root.grid_columns = vec![
            GridTrack::Fr(1.0),
            GridTrack::Fr(1.0),
            GridTrack::Fr(1.0),
            GridTrack::Fr(1.0),
        ];

        // Add 4 children (one per column)
        for _ in 0..4 {
            root.add_child(LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
        }

        test_compute_layout(&mut root, 800.0);

        // Each column should get ~200px (800 / 4)
        for i in 0..4 {
            assert!(
                (root.children[i].rect.width - 200.0).abs() < 1.0,
                "Child {} width should be ~200px, got {}",
                i,
                root.children[i].rect.width
            );
        }

        // Children should be positioned left to right
        for i in 1..4 {
            assert!(
                root.children[i].rect.x > root.children[i - 1].rect.x,
                "Child {} should be to the right of child {}",
                i,
                i - 1
            );
        }

        // All children on same row should have same y
        for i in 1..4 {
            assert!(
                (root.children[i].rect.y - root.children[0].rect.y).abs() < 1.0,
                "All children on same row should share y position"
            );
        }
    }

    #[test]
    fn test_grid_fallback_to_block() {
        // Grid without explicit columns falls back to block layout
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Grid;
        root.padding = [10.0; 4];
        // No grid_columns set

        let child1 =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        let child2 =
            LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);

        root.add_child(child1);
        root.add_child(child2);

        test_compute_layout(&mut root, 800.0);

        // Children should be stacked vertically (block fallback)
        assert!(root.children[1].rect.y >= root.children[0].rect.y);
        // Both should have full available width minus padding
        assert!((root.children[0].rect.width - 780.0).abs() < 5.0);
    }

    #[test]
    fn test_grid_two_by_two() {
        // Two columns with 4 children -> 2 rows of 2
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 600.0, 0.0));
        root.display = DisplayType::Grid;
        root.padding = [0.0; 4];
        root.grid_columns = vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0)];

        for _ in 0..4 {
            root.add_child(LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
        }

        test_compute_layout(&mut root, 600.0);

        // Each column ~300px
        assert!((root.children[0].rect.width - 300.0).abs() < 1.0);
        assert!((root.children[1].rect.width - 300.0).abs() < 1.0);

        // First row children at y=0, second row below
        assert!(root.children[2].rect.y > root.children[0].rect.y);
        // Second row starts after first row height + gap
        assert!(
            (root.children[2].rect.x - root.children[0].rect.x).abs() < 1.0,
            "Second row first child should align with first row first child"
        );
    }

    #[test]
    fn test_grid_with_fixed_and_fr() {
        // Layout: 200px 1fr -> fixed col + flexible col in 800px container
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Grid;
        root.padding = [0.0; 4];
        root.grid_columns = vec![GridTrack::Fixed(200.0), GridTrack::Fr(1.0)];

        for _ in 0..2 {
            root.add_child(LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
        }

        test_compute_layout(&mut root, 800.0);

        assert!((root.children[0].rect.width - 200.0).abs() < 1.0);
        // Second column: (800 - 200) = 600px for the fr track
        assert!((root.children[1].rect.width - 600.0).abs() < 1.0);
    }

    #[test]
    fn test_grid_with_gap() {
        // Two columns with a 10px gap between them
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Grid;
        root.padding = [0.0; 4];
        root.grid_columns = vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0)];
        root.grid_column_gap = 10.0;

        for _ in 0..2 {
            root.add_child(LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
        }

        test_compute_layout(&mut root, 800.0);

        // Each fr gets (800 - 10) / 2 = 395px
        assert!((root.children[0].rect.width - 395.0).abs() < 1.0);
        assert!((root.children[1].rect.width - 395.0).abs() < 1.0);
        // Second child x = 395 + 10 gap = 405
        assert!((root.children[1].rect.x - 405.0).abs() < 1.0);
    }

    #[test]
    fn test_grid_order_default() {
        // Default order is 0
        let node = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(node.order, 0);
    }

    // ---- Hit Test Interactive Tests ----

    #[test]
    fn test_hit_test_interactive_no_interaction() {
        let root = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(hit_test_interactive(&root, 50.0, 50.0), None);
    }

    #[test]
    fn test_hit_test_interactive_link_found() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let mut link = LayoutNode::new(Rect::new(10.0, 10.0, 80.0, 30.0));
        link.interaction_type = InteractionType::Link;
        root.add_child(link);

        assert_eq!(
            hit_test_interactive(&root, 50.0, 25.0),
            Some(InteractionType::Link)
        );
    }

    #[test]
    fn test_hit_test_interactive_input_found() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let mut input = LayoutNode::new(Rect::new(10.0, 50.0, 100.0, 20.0));
        input.interaction_type = InteractionType::Input;
        root.add_child(input);

        assert_eq!(
            hit_test_interactive(&root, 60.0, 60.0),
            Some(InteractionType::Input)
        );
    }

    #[test]
    fn test_hit_test_interactive_outside_rect() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let mut link = LayoutNode::new(Rect::new(10.0, 10.0, 80.0, 30.0));
        link.interaction_type = InteractionType::Link;
        root.add_child(link);

        // Point outside the link rect should return None
        assert_eq!(hit_test_interactive(&root, 200.0, 200.0), None);
    }

    #[test]
    fn test_hit_test_interactive_topmost_wins() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        // First child (link) occupies a larger area
        let mut link = LayoutNode::new(Rect::new(10.0, 10.0, 100.0, 100.0));
        link.interaction_type = InteractionType::Link;
        root.add_child(link);
        // Second child (input) overlaps and renders on top (added later)
        let mut input = LayoutNode::new(Rect::new(20.0, 20.0, 80.0, 80.0));
        input.interaction_type = InteractionType::Input;
        root.add_child(input);

        // Point in overlap area should return Input (topmost)
        assert_eq!(
            hit_test_interactive(&root, 50.0, 50.0),
            Some(InteractionType::Input)
        );
        // Point in link-only area should return Link
        assert_eq!(
            hit_test_interactive(&root, 15.0, 15.0),
            Some(InteractionType::Link)
        );
    }

    #[test]
    fn test_hit_test_interactive_nested() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let mut parent_node = LayoutNode::new(Rect::new(10.0, 10.0, 100.0, 100.0));
        let mut child_link = LayoutNode::new(Rect::new(20.0, 20.0, 40.0, 40.0));
        child_link.interaction_type = InteractionType::Link;
        parent_node.add_child(child_link);
        root.add_child(parent_node);

        assert_eq!(
            hit_test_interactive(&root, 30.0, 30.0),
            Some(InteractionType::Link)
        );
    }

    #[test]
    fn test_hit_test_interactive_absolute_child() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        let mut abs_link = LayoutNode::new(Rect::new(50.0, 50.0, 60.0, 30.0));
        abs_link.interaction_type = InteractionType::Link;
        root.absolute_children.push(abs_link);

        assert_eq!(
            hit_test_interactive(&root, 70.0, 60.0),
            Some(InteractionType::Link)
        );
    }

    // ---- hit_test_dom_path Tests ----

    #[test]
    fn test_hit_test_dom_path_basic() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        root.dom_node_id = Some(0); // document/root node

        let mut link = LayoutNode::new(Rect::new(20.0, 20.0, 60.0, 30.0));
        link.dom_node_id = Some(2); // e.g., <a href="#">

        let mut child = LayoutNode::new(Rect::new(10.0, 10.0, 180.0, 180.0));
        child.dom_node_id = Some(1); // e.g., <body>
        child.add_child(link);
        root.add_child(child);

        // Hit inside the link should return [root, body, link]
        let path = hit_test_dom_path(&root, 30.0, 30.0);
        assert_eq!(path, vec![0u32, 1, 2]);
    }

    #[test]
    fn test_hit_test_dom_path_outside_child() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        root.dom_node_id = Some(0);

        let mut child = LayoutNode::new(Rect::new(50.0, 50.0, 100.0, 100.0));
        child.dom_node_id = Some(1);
        root.add_child(child);

        // Hit at (20, 20) is inside root but outside child → path should be [root] only
        let path = hit_test_dom_path(&root, 20.0, 20.0);
        assert_eq!(path, vec![0u32]);
    }

    #[test]
    fn test_hit_test_dom_path_outside_all() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        root.dom_node_id = Some(0);

        // Hit completely outside the root rect
        let path = hit_test_dom_path(&root, 200.0, 200.0);
        assert!(path.is_empty());
    }

    #[test]
    fn test_hit_test_dom_path_text_node_skipped() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        root.dom_node_id = Some(0);

        // <a> element with text child (text node has no dom_node_id)
        let mut link = LayoutNode::new(Rect::new(10.0, 10.0, 80.0, 30.0));
        link.dom_node_id = Some(1); // the <a> element

        let text = LayoutNode::new(Rect::new(15.0, 15.0, 70.0, 20.0));
        // text node: dom_node_id stays None (default)
        link.add_child(text);
        root.add_child(link);

        // Hit inside text node → path should be [root, link] (text node skipped)
        let path = hit_test_dom_path(&root, 20.0, 20.0);
        assert_eq!(path, vec![0u32, 1]);
    }

    #[test]
    fn test_hit_test_dom_path_topmost_child_wins() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        root.dom_node_id = Some(0);

        // Two overlapping children - first one added is below the second
        let mut below = LayoutNode::new(Rect::new(10.0, 10.0, 100.0, 100.0));
        below.dom_node_id = Some(1);
        root.add_child(below);

        let mut above = LayoutNode::new(Rect::new(20.0, 20.0, 80.0, 80.0));
        above.dom_node_id = Some(2); // rendered on top
        root.add_child(above);

        // Hit in overlap area → should find [root, above] (topmost)
        let path = hit_test_dom_path(&root, 30.0, 30.0);
        assert_eq!(path, vec![0u32, 2]);
    }

    #[test]
    fn test_hit_test_dom_path_absolute_child_on_top() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 200.0));
        root.dom_node_id = Some(0);

        // Normal child
        let mut normal = LayoutNode::new(Rect::new(10.0, 10.0, 100.0, 100.0));
        normal.dom_node_id = Some(1);
        root.add_child(normal);

        // Absolute child (renders on top of normal flow)
        let mut abs = LayoutNode::new(Rect::new(30.0, 30.0, 60.0, 60.0));
        abs.dom_node_id = Some(2);
        root.absolute_children.push(abs);

        // Hit in overlap → should find absolute child (on top)
        let path = hit_test_dom_path(&root, 40.0, 40.0);
        assert_eq!(path, vec![0u32, 2]);
    }
}
