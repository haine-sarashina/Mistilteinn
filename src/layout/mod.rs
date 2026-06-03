/// Layout engine module.
///
/// Responsible for:
/// - Box model computation (content, padding, border, margin)
/// - Flexbox layout
/// - Inline text layout
/// - Render tree construction

use crate::css::{AlignItems, ComputedValues, DisplayType, FlexDirection, FlexWrap, JustifyContent};
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
        self.margin[1] + self.margin[3] + self.border[1] + self.border[3] + self.padding[1] + self.padding[3]
    }

    fn vertical_margin_and_padding_and_border(&self) -> f32 {
        self.margin[0] + self.margin[2] + self.border[0] + self.border[2] + self.padding[0] + self.padding[2]
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
    let root_styles = styles
        .get(&root_id)
        .cloned()
        .unwrap_or_default();

    let root_dom = get_node(root_id);
    let mut root_layout = LayoutNode::new_with_display(
        Rect::new(0.0, 0.0, _page_width, 0.0),
        root_styles.display,
    );
    root_layout.padding = root_styles.padding;
    root_layout.margin = root_styles.margin;
    root_layout.flex_direction = root_styles.flex_direction;
    root_layout.flex_wrap = root_styles.flex_wrap;
    root_layout.justify_content = root_styles.justify_content;
    root_layout.align_items = root_styles.align_items;

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
                    let display = if matches!(child_styles.display, DisplayType::Flex | DisplayType::InlineFlex) {
                        child_styles.display
                    } else {
                        DisplayType::Block
                    };
                    let mut layout_node = LayoutNode::new_with_display(
                        Rect::new(0.0, 0.0, 0.0, 0.0),
                        display,
                    );
                    layout_node.padding = child_styles.padding;
                    layout_node.margin = child_styles.margin;
                    layout_node.background_color = child_styles.background_color;
                    // Copy flexbox properties
                    layout_node.flex_direction = child_styles.flex_direction;
                    layout_node.flex_wrap = child_styles.flex_wrap;
                    layout_node.justify_content = child_styles.justify_content;
                    layout_node.align_items = child_styles.align_items;
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
    let available_width = page_width - root.margin[3] - root.border[3] - root.padding[3]
        - root.padding[1] - root.border[1] - root.margin[1];

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
        let child_height = compute_block_height(child, depth + 1) + child.padding[0] + child.padding[2]
            + child.border[0] + child.border[2];

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
            compute_block_height(child, depth + 1) + child.padding[0] + child.padding[2]
                + child.border[0] + child.border[2]
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
        return if is_row { item.rect.width } else { item.rect.height };
    }
    // Estimate from children: use block height computation as fallback
    if is_row {
        compute_block_height(item, 0)
            + item.padding[0] + item.padding[2]
            + item.border[0] + item.border[2]
    } else {
        // For column direction, we can't easily estimate width from children alone
        // Return 0 and let flex-grow handle distribution
        0.0
    }
}

/// Compute the cross-axis size of a flex item.
fn compute_flex_cross_size(item: &LayoutNode) -> f32 {
    if item.rect.height > 0.0 {
        return item.rect.height - item.padding[0] - item.padding[2]
            - item.border[0] - item.border[2];
    }
    compute_block_height(item, 0) + item.padding[0] + item.padding[2]
        + item.border[0] + item.border[2]
}

/// Layout flex children according to CSS Flexbox spec (simplified single-line).
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

    let is_row = matches!(parent.flex_direction, FlexDirection::Row | FlexDirection::RowReverse);
    let is_reverse = matches!(
        parent.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );

    if parent.children.is_empty() {
        return;
    }

    // Step 1: Resolve flex basis for each item
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

    // Step 2: Compute container main size and free space
    let container_main_size = if is_row {
        available_width
    } else {
        f32::MAX // column: no fixed constraint (auto height)
    };

    let total_main: f32 = states
        .iter()
        .map(|s| s.basis + s.main_margin)
        .sum();
    let free_space = container_main_size - total_main;

    // Step 3: Distribute positive free space (flex-grow)
    if free_space > 0.0 {
        let total_grow: f32 = parent
            .children
            .iter()
            .filter(|c| c.flex_grow > 0.0)
            .map(|c| c.flex_grow)
            .sum();

        if total_grow > 0.0 {
            for (i, child) in parent.children.iter().enumerate() {
                if child.flex_grow > 0.0 {
                    let share = free_space * (child.flex_grow / total_grow);
                    states[i].basis += share;
                }
            }
        }
    }

    // Step 4: Distribute negative free space (flex-shrink)
    if free_space < 0.0 {
        let total_shrink_weight: f32 = parent
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.flex_shrink > 0.0)
            .map(|(i, c)| c.flex_shrink * states[i].basis)
            .sum();

        if total_shrink_weight > 0.0 {
            for (i, child) in parent.children.iter().enumerate() {
                if child.flex_shrink > 0.0 {
                    let shrink_amount = (-free_space)
                        * (child.flex_shrink * states[i].basis)
                        / total_shrink_weight;
                    states[i].basis = (states[i].basis - shrink_amount).max(0.0);
                }
            }
        }
    }

    // Step 5: Position children on the main axis
    let mut current_main_pos = parent_x;
    let mut max_cross_size: f32 = 0.0;

    for (i, child) in parent.children.iter_mut().enumerate() {
        let main_size = (states[i].basis - states[i].main_margin).max(0.0);

        if is_row {
            // Row direction: main axis = horizontal
            let cross_size = compute_flex_cross_size(child);
            child.rect.x = current_main_pos + child.margin[3];
            child.rect.y = parent_y + child.margin[0];
            child.rect.width = main_size;
            child.rect.height = cross_size + child.margin[0] + child.margin[2];

            let total_cross = child.rect.height + child.margin[0] + child.margin[2];
            if total_cross > max_cross_size {
                max_cross_size = total_cross;
            }

            let step = main_size + states[i].main_margin;
            if is_reverse {
                current_main_pos -= step;
            } else {
                current_main_pos += step;
            }
        } else {
            // Column direction: main axis = vertical
            let child_width = (available_width - states[i].main_margin).max(0.0);
            child.rect.x = parent_x + child.margin[3];
            child.rect.y = current_main_pos + child.margin[0];
            child.rect.width = child_width;
            child.rect.height = main_size;

            let step = main_size + states[i].main_margin;
            if is_reverse {
                current_main_pos -= step;
            } else {
                current_main_pos += step;
            }
        }
    }

    // Step 6: Apply justify-content (main-axis distribution)
    if is_row {
        let total_items_width: f32 = parent
            .children
            .iter()
            .map(|c| c.rect.width + c.margin[3] + c.margin[1])
            .sum();
        let justify_space = (available_width - total_items_width).max(0.0);

        if justify_space > 0.0 {
            match parent.justify_content {
                JustifyContent::FlexStart => {} // already at start
                JustifyContent::FlexEnd => {
                    for child in &mut parent.children {
                        child.rect.x += justify_space;
                    }
                }
                JustifyContent::Center => {
                    let offset = justify_space / 2.0;
                    for child in &mut parent.children {
                        child.rect.x += offset;
                    }
                }
                JustifyContent::SpaceBetween => {
                    if parent.children.len() > 1 {
                        let gap = justify_space / (parent.children.len() - 1) as f32;
                        for (j, child) in parent.children.iter_mut().enumerate() {
                            child.rect.x += gap * j as f32;
                        }
                    }
                }
                JustifyContent::SpaceAround => {
                    let n = parent.children.len();
                    if n > 0 {
                        let gap = justify_space / n as f32;
                        for (j, child) in parent.children.iter_mut().enumerate() {
                            child.rect.x += gap * j as f32 + gap / 2.0;
                        }
                    }
                }
            }
        }
    }

    // Step 7: Apply align-items (cross-axis alignment)
    if is_row {
        let container_cross = max_cross_size + parent.padding[0] + parent.padding[2];
        match parent.align_items {
            AlignItems::Stretch => {
                for child in &mut parent.children {
                    let stretch_h = (container_cross
                        - parent.padding[0]
                        - parent.padding[2]
                        - child.margin[0]
                        - child.margin[2])
                        .max(0.0);
                    if child.flex_basis.is_none() || child.rect.height <= 0.0 {
                        // Only stretch items without explicit height
                    } else {
                        let current = child.rect.height;
                        child.rect.height = stretch_h.max(current);
                    }
                }
            }
            AlignItems::FlexStart => {} // already at top
            AlignItems::FlexEnd => {
                for child in &mut parent.children {
                    let offset = container_cross
                        - child.rect.height
                        - parent.padding[0]
                        - child.margin[0]
                        - child.margin[2];
                    if offset > 0.0 {
                        child.rect.y += offset;
                    }
                }
            }
            AlignItems::Center => {
                for child in &mut parent.children {
                    let used = child.rect.height + child.margin[0] + child.margin[2];
                    let offset = (container_cross - used) / 2.0;
                    if offset > 0.0 {
                        child.rect.y += offset;
                    }
                }
            }
        }
    }

    // Step 8: Recurse into children
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

    // Update parent height based on children extent
    let mut max_bottom = parent_y;
    for child in &parent.children {
        let bottom = child.rect.bottom() + child.margin[2];
        if bottom > max_bottom {
            max_bottom = bottom;
        }
    }
    let content_height = (max_bottom - parent_y).max(0.0);
    parent.rect.height = content_height + parent.padding[0] + parent.border[0]
        + parent.padding[2] + parent.border[2];
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
        let node = LayoutNode::new_with_display(Rect::new(0.0, 0.0, 100.0, 100.0), DisplayType::Inline);
        assert_eq!(node.display, DisplayType::Inline);
    }

    #[test]
    fn compute_block_layout_stacking() {
        let mut root = LayoutNode::new(Rect::new(0.0, 0.0, 800.0, 600.0));
        root.padding = [10.0; 4];

        let child1 = LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
        root.add_child(child1);
        let child2 = LayoutNode::new_with_display(Rect::new(0.0, 0.0, 0.0, 0.0), DisplayType::Block);
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
}
