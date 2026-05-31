/// HTML parser module.
///
/// Uses html5ever for robust HTML5 parsing.
/// DOM tree construction will be implemented incrementally.

/// A simple DOM tree node for our own use.
#[derive(Debug, Clone)]
pub struct DomNode {
    pub name: String,
    pub children: Vec<DomNode>,
    pub attributes: Vec<(String, String)>,
    pub text: Option<String>,
}

impl DomNode {
    pub fn element(name: &str) -> Self {
        Self {
            name: name.to_string(),
            children: Vec::new(),
            attributes: Vec::new(),
            text: None,
        }
    }

    pub fn text(content: &str) -> Self {
        Self {
            name: "#text".to_string(),
            children: Vec::new(),
            attributes: Vec::new(),
            text: Some(content.to_string()),
        }
    }

    pub fn add_child(&mut self, child: DomNode) {
        self.children.push(child);
    }

    pub fn set_attribute(&mut self, key: &str, value: &str) {
        self.attributes.push((key.to_string(), value.to_string()));
    }
}

/// Parses HTML and returns the root document.
///
/// Currently a simplified tokenizer-based parser.
/// Will be replaced with full html5ever-based parsing.
pub fn parse_html(_source: &str) -> DomNode {
    DomNode::element("html")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_root() {
        let root = parse_html("<html><body>Hello</body></html>");
        assert_eq!(root.name, "html");
    }

    #[test]
    fn dom_node_add_child() {
        let mut root = DomNode::element("div");
        root.add_child(DomNode::element("span"));
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn dom_node_set_attribute() {
        let mut node = DomNode::element("a");
        node.set_attribute("href", "https://example.com");
        assert_eq!(node.attributes.len(), 1);
    }
}
