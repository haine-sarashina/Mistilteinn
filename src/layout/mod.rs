/// Layout engine module.
///
/// Responsible for:
/// - Box model computation (content, padding, border, margin)
/// - Flexbox layout
/// - Inline text layout
/// - Render tree construction

use crate::css::{ComputedValues, DisplayType};

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
    fn tag_name(&self) -> &str;
    fn get_attr(&self, name: &str) -> Option<String>;
    fn children_ids(&self) -> Vec<u32>;
    fn text_content(&self) -> Option<&str>;
}

/// Build a layout tree from DOM nodes and computed styles.
///
/// Takes a root node ID, a map of node_id -> ComputedValues,
/// and a way to look up DOM nodes by ID.
pub fn build_layout_tree<N>(
    root_id: u32,
    styles: &std::collections::HashMap<u32, ComputedValues>,
    get_node: &impl Fn(u32) -> Option<N>,
    _page_width: f32,
) -> LayoutNode
where
    N: LayoutDomNode,
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

    if let Some(ref node) = root_dom {
        build_layout_children(&mut root_layout, &node.children_ids(), styles, get_node, 0);
    }

    root_layout
}

/// Maximum nesting depth for layout tree traversal.
/// Real-world pages rarely exceed 50 levels; 512 provides a large safety margin
/// while preventing stack overflow on pathologically deep DOM trees.
const MAX_LAYOUT_DEPTH: usize = 512;

fn build_layout_children<N>(
    parent: &mut LayoutNode,
    child_ids: &[u32],
    styles: &std::collections::HashMap<u32, ComputedValues>,
    get_node: impl Fn(u32) -> Option<N>,
    depth: usize,
) where
    N: LayoutDomNode,
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
                DisplayType::Block => {
                    let mut layout_node = LayoutNode::new_with_display(
                        Rect::new(0.0, 0.0, 0.0, 0.0),
                        DisplayType::Block,
                    );
                    layout_node.padding = child_styles.padding;
                    layout_node.margin = child_styles.margin;

                    let text = node.text_content().map(|t| t.to_string());
                    if text.is_some() && node.children_ids().is_empty() {
                        layout_node.text = text;
                    } else {
                        build_layout_children(
                            &mut layout_node,
                            &node.children_ids(),
                            styles,
                            &get_node,
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

                    let text = node.text_content().map(|t| t.to_string());
                    if text.is_some() && node.children_ids().is_empty() {
                        layout_node.text = text;
                    } else {
                        build_layout_children(
                            &mut layout_node,
                            &node.children_ids(),
                            styles,
                            &get_node,
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
/// Inline children are laid out horizontally within a line.
/// Each node's margin, border, padding, and content box are computed.
pub fn compute_layout(root: &mut LayoutNode, page_width: f32) {
    let available_width = page_width - root.margin[3] - root.border[3] - root.padding[3]
        - root.padding[1] - root.border[1] - root.margin[1];

    compute_block_children(
        root,
        root.padding[3] + root.border[3],
        root.padding[0] + root.border[0],
        available_width,
        0,
    );
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
}
