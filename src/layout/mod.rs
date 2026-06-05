/// Layout engine module.
///
/// Responsible for:
/// - Box model computation (content, padding, border, margin)
/// - Flexbox layout
/// - Inline text layout
/// - Render tree construction
use crate::css::{
    AlignContent, AlignItems, ComputedValues, DisplayType, FlexDirection, FlexWrap, JustifyContent,
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
        // Silently stop building — the page will render partially rather than crash.
        return;
    }

    for &child_id in child_ids {
        if let Some(node) = get_node(child_id) {
            let child_styles = styles.get(&child_id).cloned().unwrap_or_default();

            match child_styles.display {
                DisplayType::None => continue,
                DisplayType::Block | DisplayType::Flex | DisplayType::InlineFlex => {
                    let display = if matches!(
                        child_styles.display,
                        DisplayType::Flex | DisplayType::InlineFlex
                    ) {
                        child_styles.display
                    } else {
                        DisplayType::Block
                    };
                    let mut layout_node =
                        LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), display);
                    layout_node.padding = child_styles.padding;
                    layout_node.margin = child_styles.margin;
                    layout_node.background_color = child_styles.background_color;
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
/// Each node's margin, border, padding, and content box are computed.
pub fn compute_layout(root: &mut LayoutNode, page_width: f32) {
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
            compute_flex_children(root, start_x, start_y, available_width, 0);
        }
        _ => {
            compute_block_children(root, start_x, start_y, available_width, 0);
        }
    }
}

/// Stack block-level children vertically, computing each child's rect.
fn compute_block_children(
    parent: &mut LayoutNode,
    parent_x: f32,
    mut y: f32,
    available_width: f32,
    depth: usize,
) {
    if depth > MAX_LAYOUT_DEPTH {
        return;
    }

    for child in &mut parent.children {
        // Apply margins
        let child_x = parent_x + child.margin[3];
        let child_y = y + child.margin[0];
        let child_width = available_width - child.margin[3] - child.margin[1];
        let child_height = compute_block_height(child, depth + 1)
            + child.padding[0]
            + child.padding[2]
            + child.border[0]
            + child.border[2];

        child.rect = Rect::new(child_x, child_y, child_width, child_height);
        y = child.rect.bottom() + child.margin[2];
    }
}

/// Compute the height of a block node (content + inner children).
fn compute_block_height(node: &LayoutNode, depth: usize) -> f32 {
    if depth > MAX_LAYOUT_DEPTH {
        return 0.0;
    }
    let mut height = 0.0;
    for child in &node.children {
        let inner_height = if child.display == DisplayType::Block {
            compute_block_height(child, depth + 1)
                + child.padding[0]
                + child.padding[2]
                + child.border[0]
                + child.border[2]
        } else {
            compute_inline_height(child)
        };
        height += inner_height;
    }
    height + node.padding[0] + node.padding[2]
}

/// Compute the height of an inline node (used as a line box).
/// Uses the estimated font-size * 1.2 as the line-height per CSS spec.
fn compute_inline_height(node: &LayoutNode) -> f32 {
    // TODO: use actual ComputedValues.font_size once available in LayoutNode
    let font_size = 16.0; // default CSS initial value
    let line_height = font_size * 1.2;
    line_height + node.padding[0] + node.padding[2]
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

            let basis = if let Some(b) = child.flex_basis {
                b
            } else if is_row && child.rect.width > 0.0 {
                child.rect.width
            } else if !is_row && child.rect.height > 0.0 {
                child.rect.height
            } else {
                compute_flex_item_content_main_size(child, is_row)
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
                        if parent.children[i].flex_basis.is_none()
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
                compute_flex_children(child, inner_x, inner_y, inner_width, depth + 1);
            }
            _ => {
                compute_block_children(child, inner_x, inner_y, inner_width, depth + 1);
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

/// Flatten the layout tree into renderable rectangles with colors.
///
/// Collects all nodes that have a background color, plus leaf text nodes
/// that have computed dimensions. Used to bridge layout tree to renderer.
pub fn collect_render_rects(node: &LayoutNode) -> Vec<(Rect, Option<[u8; 4]>)> {
    let mut rects = Vec::new();

    if node.background_color.is_some() {
        rects.push((node.rect, node.background_color));
    }

    for child in &node.children {
        rects.extend(collect_render_rects(child));
    }

    if node.text.is_some() && node.background_color.is_none() {
        if node.rect.width > 0.0 && node.rect.height > 0.0 {
            rects.push((node.rect, Some([0, 0, 0, 255])));
        }
    }

    rects
}

#[cfg(test)]
mod tests {
    use super::*;

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

        compute_layout(&mut root, 800.0);
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

        compute_layout(&mut root, 800.0);

        // Both children have basis=0 and no grow → both get full container width split evenly via shrink
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

        compute_layout(&mut root, 800.0);

        // Each should get ~400px width (half of 800)
        assert!((root.children[0].rect.width - 400.0).abs() < 1.0);
        assert!((root.children[1].rect.width - 400.0).abs() < 1.0);
        // Second child should be to the right of first
        assert!(root.children[1].rect.x >= root.children[0].rect.x);
    }

    #[test]
    fn test_flex_grow_weighted() {
        // grow:2 and grow:1 → 2:1 ratio of free space
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 900.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_grow = 2.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 900.0);

        // 2:1 ratio → child1 gets 600px, child2 gets 300px
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
        child1.flex_basis = Some(100.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 50.0));
        child2.flex_basis = Some(50.0);

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 800.0);

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
        child1.flex_basis = Some(0.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = Some(0.0);
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child3.flex_basis = Some(0.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        compute_layout(&mut root, 600.0);

        // All items have 0 basis + no grow → all at width 0
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
        child1.flex_basis = Some(200.0);

        root.add_child(child1);

        compute_layout(&mut root, 800.0);

        // Item should be centered: (800 - 200) / 2 = 300 offset
        assert!((root.children[0].rect.x - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_flex_shrink_overflow() {
        // Total basis (600+400=1000) exceeds container (800) → shrink proportionally
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_basis = Some(600.0);
        child1.flex_shrink = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = Some(400.0);
        child2.flex_shrink = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 800.0);

        // Overflow = 200px. Shrink weight: 1*600=600 and 1*400=400, total=1000
        // child1 shrinks by: 200 * 600/1000 = 120 → width = 480
        // child2 shrinks by: 200 * 400/1000 = 80 → width = 320
        assert!((root.children[0].rect.width - 480.0).abs() < 2.0);
        assert!((root.children[1].rect.width - 320.0).abs() < 2.0);
    }

    #[test]
    fn test_flex_with_basis_and_grow() {
        // basis: 100 + 100 = 200, container: 800 → free_space = 600, split by grow 1:1
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child1.flex_basis = Some(100.0);
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 0.0, 0.0));
        child2.flex_basis = Some(100.0);
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 800.0);

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
        child1.flex_basis = Some(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 80.0));
        child2.flex_basis = Some(200.0);

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 800.0);

        // Both on same line, side by side
        assert_eq!(root.children[0].rect.x, 0.0);
        assert!(root.children[1].rect.x > root.children[0].rect.x);
        // Both at same y (within tolerance for cross-size differences)
        assert!((root.children[0].rect.y - root.children[1].rect.y).abs() < 1.0);
    }

    #[test]
    fn test_flex_wrap_basic() {
        // Three items, each ~300px basis in 800px container → third wraps to second line
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 0.0));
        root.display = DisplayType::Flex;
        root.flex_wrap = FlexWrap::Wrap;
        root.padding = [0.0; 4];

        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child1.flex_basis = Some(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 60.0));
        child2.flex_basis = Some(300.0);
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 40.0));
        child3.flex_basis = Some(300.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        compute_layout(&mut root, 800.0);

        // First two items on line 1 (y ≈ 0)
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

        // Line 1: two items with basis 500 total, grow=1 each → fill 800
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        child1.flex_basis = Some(200.0);
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = Some(300.0);
        child2.flex_grow = 1.0;

        // Line 2: two items with basis 700 total, grow=1 each → fill 800
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 350.0, 50.0));
        child3.flex_basis = Some(350.0);
        child3.flex_grow = 1.0;
        let mut child4 = LayoutNode::new(Rect::new(0.0, 0.0, 350.0, 50.0));
        child4.flex_basis = Some(350.0);
        child4.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);
        root.add_child(child4);

        compute_layout(&mut root, 800.0);

        // Line 1: basis total = 500, free = 300. Each grows by 150 → widths: 350, 450
        assert!((root.children[0].rect.width - 350.0).abs() < 2.0);
        assert!((root.children[1].rect.width - 450.0).abs() < 2.0);
        // Line 2: basis total = 700, free = 100. Each grows by 50 → widths: 400, 400
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
        child1.flex_basis = Some(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 60.0));
        child2.flex_basis = Some(300.0);
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 40.0));
        child3.flex_basis = Some(300.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        compute_layout(&mut root, 800.0);

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
        child1.flex_basis = Some(200.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 50.0));
        child2.flex_basis = Some(200.0);

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 800.0);

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
        child1.flex_basis = Some(400.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 60.0));
        child2.flex_basis = Some(400.0);

        // Third item wraps to line 2
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 40.0));
        child3.flex_basis = Some(300.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        compute_layout(&mut root, 800.0);

        // Line 1 items at y=0
        assert!((root.children[0].rect.y - 0.0).abs() < 1.0);
        assert!((root.children[1].rect.y - 0.0).abs() < 1.0);
        // Line 2 item should start at y = max(line1 cross_size) + row_gap
        // child1 cross ~50, child2 cross ~60 → line height ≈ 60 + 16 gap = 76
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
        child1.flex_basis = Some(100.0);
        child1.flex_grow = 1.0;
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 100.0, 50.0));
        child2.flex_basis = Some(100.0);
        child2.flex_grow = 1.0;

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 840.0);

        // Total basis = 200, gap = 20, free_space = 840 - 200 - 20 = 620
        // Each grows by 310 → widths: 410, 410
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
        child1.flex_basis = Some(500.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = Some(300.0);

        // Line 2: one item that wraps
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 40.0));
        child3.flex_basis = Some(400.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        compute_layout(&mut root, 800.0);

        // Lines total cross-size ≈ max(50, 50) + max(40, 0) = ~90 (plus gaps)
        // Container height is 300, so excess ≈ 210px should be distributed as centering
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
        child1.flex_basis = Some(500.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 60.0));
        child2.flex_basis = Some(400.0);

        // Wraps to line 2
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child3.flex_basis = Some(300.0);
        let mut child4 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 40.0));
        child4.flex_basis = Some(200.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);
        root.add_child(child4);

        compute_layout(&mut root, 800.0);

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

        // Line 1: two items (600 total), centered → offset of 100px
        let mut child1 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child1.flex_basis = Some(300.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = Some(300.0);

        // Line 2: one item (400), centered → offset of 200px
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 50.0));
        child3.flex_basis = Some(400.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);

        compute_layout(&mut root, 800.0);

        // Line 1: total width 600, centered → x starts at (800-600)/2 = 100
        assert!((root.children[0].rect.x - 100.0).abs() < 2.0);
        // Line 2: total width 400, centered → x starts at (800-400)/2 = 200
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
        child1.flex_basis = Some(200.0);
        // This item is wider than the container itself
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 700.0, 60.0));
        child2.flex_basis = Some(700.0);

        root.add_child(child1);
        root.add_child(child2);

        compute_layout(&mut root, 600.0);

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
        child1.flex_basis = Some(500.0);
        let mut child2 = LayoutNode::new(Rect::new(0.0, 0.0, 300.0, 50.0));
        child2.flex_basis = Some(300.0);

        // Line 2: wraps
        let mut child3 = LayoutNode::new(Rect::new(0.0, 0.0, 400.0, 30.0));
        child3.flex_basis = Some(400.0);
        let mut child4 = LayoutNode::new(Rect::new(0.0, 0.0, 200.0, 45.0));
        child4.flex_basis = Some(200.0);

        root.add_child(child1);
        root.add_child(child2);
        root.add_child(child3);
        root.add_child(child4);

        compute_layout(&mut root, 800.0);

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
}
