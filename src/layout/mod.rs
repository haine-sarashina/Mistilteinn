/// Layout engine module.
///
/// Responsible for:
/// - Box model computation (content, padding, border, margin)
/// - Flexbox layout
/// - Inline text layout
/// - Render tree construction

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x
            && x < self.x + self.width
            && y >= self.y
            && y < self.y + self.height
    }
}

/// A node in the layout tree.
#[derive(Debug)]
pub struct LayoutNode {
    pub rect: Rect,
    pub children: Vec<LayoutNode>,
}

impl LayoutNode {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            children: Vec::new(),
        }
    }

    pub fn add_child(&mut self, child: LayoutNode) {
        self.children.push(child);
    }
}

/// Computes the layout tree from the DOM + styles.
///
/// Placeholder — will be implemented with box model, flexbox, and inline layout.
pub fn compute_layout(_root: &LayoutNode) -> LayoutNode {
    LayoutNode::new(Rect::new(0.0, 0.0, 1280.0, 800.0))
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
}
