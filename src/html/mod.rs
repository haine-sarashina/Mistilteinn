pub mod parser;

/// Parse HTML source into a \[DomArena\].
///
/// Re-export of \[parser::parse_html\].
#[allow(unused_imports)]
pub use parser::parse_html;

use html5ever::interface::QualName;
use html5ever::tree_builder::{Attribute, ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{LocalName, Namespace, namespace_url, ns};
use markup5ever::tendril::StrTendril;
use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::cell::RefCell;

// ------ Node Types ------

/// The kind of a DOM node.
#[derive(Debug, Clone)]
pub enum DomNodeType {
    Document,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    Element {
        name: LocalName,
        namespace: Namespace,
    },
    Text(String),
    Comment(String),
}

/// A single DOM node.
#[derive(Debug, Clone)]
pub struct DomNode {
    pub node_type: DomNodeType,
    pub attrs: FxHashMap<LocalName, String>,
    #[cfg_attr(feature = "memprof", allow(dead_code))]
    pub(crate) children: Vec<NodeId>,
    #[cfg_attr(feature = "memprof", allow(dead_code))]
    pub(crate) parent: Option<NodeId>,
    /// For `<template>` elements: handle of the implicit content fragment.
    pub(crate) template_content: Option<NodeId>,
}

impl DomNode {
    pub fn is_element(&self) -> bool {
        matches!(self.node_type, DomNodeType::Element { .. })
    }

    /// Get the parent node's ID, if one exists.
    pub fn parent_id(&self) -> Option<u32> {
        self.parent.map(|p| p.0)
    }

    pub fn is_text(&self) -> bool {
        matches!(self.node_type, DomNodeType::Text(_))
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.node_type, DomNodeType::Comment(_))
    }

    pub fn tag_name(&self) -> Option<&LocalName> {
        match &self.node_type {
            DomNodeType::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn get_attr(&self, name: &str) -> Option<&str> {
        let key = LocalName::from(name);
        self.attrs.get(&key).map(|v| v.as_str())
    }

    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.get_attr(name)
    }

    pub fn attr_iter(&self) -> impl Iterator<Item = (&LocalName, &str)> {
        self.attrs.iter().map(|(k, v)| (k, v.as_str()))
    }

    // -- Legacy helpers for non-arena tests --

    /// Child nodes (only meaningful for arena-free / test use).
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    pub fn add_child(&mut self, _child: NodeId) {
        // Arena-only; this is for legacy test compat.
    }

    pub fn set_attribute(&mut self, key: &str, value: &str) {
        self.attrs.insert(LocalName::from(key), value.to_string());
    }

    pub fn element(name: &str) -> Self {
        Self {
            node_type: DomNodeType::Element {
                name: LocalName::from(name),
                namespace: ns!(html),
            },
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: None,
            template_content: None,
        }
    }

    pub fn text(content: &str) -> Self {
        Self {
            node_type: DomNodeType::Text(content.to_string()),
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: None,
            template_content: None,
        }
    }

    /// Get the text content of this node if it is a text node.
    pub fn text_content(&self) -> Option<&str> {
        match &self.node_type {
            DomNodeType::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Bridge between the layout engine and DOM nodes.
impl crate::layout::LayoutDomNode for DomNode {
    fn tag_name(&self) -> String {
        match &self.node_type {
            DomNodeType::Element { name, .. } => name.to_string(),
            _ => String::new(),
        }
    }

    fn get_attr(&self, name: &str) -> Option<String> {
        self.attrs.get(&LocalName::from(name)).cloned()
    }

    fn children_ids(&self) -> Vec<u32> {
        self.children.iter().map(|nid| nid.0).collect()
    }

    fn text_content(&self) -> Option<&str> {
        match &self.node_type {
            DomNodeType::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn attributes(&self) -> Vec<(String, String)> {
        self.attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }
}

// ------ Handle / Id ------

/// Stable index into the arena. ID 0 is the document node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub const DOCUMENT: Self = Self(0);

    pub fn index(&self) -> usize {
        self.0 as usize
    }

    /// Create a NodeId from a raw u32 index.
    pub fn from_raw(id: u32) -> Self {
        Self(id)
    }
}

/// A handle into the DOM arena (clone = copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomHandle(pub NodeId);

// ------ ElemName ------

/// Owned element name (returned by `elem_name()`).
#[derive(Debug, Clone)]
pub struct OwnedElemName {
    ns: Namespace,
    local: LocalName,
}

impl html5ever::tree_builder::ElemName for OwnedElemName {
    fn ns(&self) -> &Namespace {
        &self.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.local
    }
}

// ------ Arena ------

/// Arena-backed DOM storage with interior mutability.
#[derive(Clone)]
pub struct DomArena {
    #[doc(hidden)]
    #[cfg_attr(test, allow(dead_code))]
    pub nodes: RefCell<Vec<DomNode>>,
}

impl DomArena {
    pub fn new() -> Self {
        Self {
            nodes: RefCell::new(vec![DomArena::make_document()]),
        }
    }

    fn make_document() -> DomNode {
        DomNode {
            node_type: DomNodeType::Document,
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: None,
            template_content: None,
        }
    }

    fn alloc(&self, node: DomNode) -> NodeId {
        let mut nodes = self.nodes.borrow_mut();
        let id = nodes.len() as u32;
        nodes.push(node);
        NodeId(id)
    }

    fn with_node<R, F>(&self, id: NodeId, f: F) -> Option<R>
    where
        F: FnOnce(&DomNode) -> R,
    {
        let nodes = self.nodes.borrow();
        nodes.get(id.index()).map(f)
    }

    fn with_node_mut<R, F>(&self, id: NodeId, f: F) -> Option<R>
    where
        F: FnOnce(&mut DomNode) -> R,
    {
        self.nodes.borrow_mut().get_mut(id.index()).map(f)
    }

    /// Public: get a node by handle.
    pub fn get(&self, h: DomHandle) -> Option<DomNode> {
        self.with_node(h.0, |n| n.clone())
    }

    /// Number of nodes in the arena.
    pub fn len(&self) -> usize {
        self.nodes.borrow().len()
    }

    /// Root document handle.
    pub fn document_handle(&self) -> DomHandle {
        DomHandle(NodeId::DOCUMENT)
    }

    /// Find a node by `id` attribute.
    pub fn find_by_id(&self, id_val: &str) -> Option<u32> {
        let nodes = self.nodes.borrow();
        for (i, node) in nodes.iter().enumerate() {
            if let Some(val) = node.get_attr("id") {
                if val == id_val {
                    return Some(i as u32);
                }
            }
        }
        None
    }

    /// Find the first element node matching a tag name.
    pub fn find_by_tag(&self, tag: &str) -> Option<u32> {
        let tag_name = LocalName::from(tag);
        let nodes = self.nodes.borrow();
        for (i, node) in nodes.iter().enumerate() {
            if let Some(t) = node.tag_name() {
                if t == &tag_name {
                    return Some(i as u32);
                }
            }
        }
        None
    }

    /// Extract text content of a node and its children.
    pub fn get_text_content(&self, node_id: u32) -> String {
        let mut text = String::new();
        self.collect_text_recursive(node_id, &mut text);
        text
    }

    fn collect_text_recursive(&self, node_id: u32, out: &mut String) {
        let nodes = self.nodes.borrow();
        if let Some(node) = nodes.get(node_id as usize) {
            match &node.node_type {
                DomNodeType::Text(s) => {
                    out.push_str(s);
                }
                _ => {
                    let children = node.children.clone();
                    drop(nodes);
                    for child in children {
                        self.collect_text_recursive(child.0, out);
                    }
                }
            }
        }
    }

    /// Set the text content of a node by replacing its children with a single text node.
    pub fn set_text_content(&self, node_id: u32, new_text: &str) {
        let text_node = DomNode {
            node_type: DomNodeType::Text(new_text.to_string()),
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: Some(NodeId(node_id)),
            template_content: None,
        };
        let new_text_id = self.alloc(text_node);
        let mut nodes = self.nodes.borrow_mut();
        if let Some(node) = nodes.get_mut(node_id as usize) {
            node.children.clear();
            node.children.push(new_text_id);
        }
    }

    /// Set an attribute on a node.
    pub fn set_attribute(&self, node_id: u32, name: &str, val: &str) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(node) = nodes.get_mut(node_id as usize) {
            node.set_attribute(name, val);
        }
    }

    /// Get an attribute on a node.
    pub fn get_attribute(&self, node_id: u32, name: &str) -> Option<String> {
        let nodes = self.nodes.borrow();
        nodes
            .get(node_id as usize)
            .and_then(|n| n.get_attr(name).map(|s| s.to_string()))
    }

    /// Extract all inline `<script>` contents from the DOM.
    /// Only extracts executable JavaScript scripts (skips application/ld+json, text/template, etc.).
    pub fn extract_scripts(&self) -> Vec<String> {
        let mut scripts = Vec::new();
        let script_tag = LocalName::from("script");
        let count = self.len();
        for i in 0..count {
            let is_js_script = {
                let nodes = self.nodes.borrow();
                if let Some(node) = nodes.get(i) {
                    if node.tag_name().map_or(false, |t| t == &script_tag) {
                        if let Some(type_attr) = node.get_attr("type") {
                            let t = type_attr.trim().to_lowercase();
                            t.is_empty()
                                || t == "text/javascript"
                                || t == "application/javascript"
                                || t == "text/ecmascript"
                                || t == "application/ecmascript"
                                || t == "module"
                        } else {
                            true
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if is_js_script {
                let text = self.get_text_content(i as u32);
                if !text.trim().is_empty() {
                    scripts.push(text);
                }
            }
        }
        scripts
    }

    /// Extract the `<title>` text content from the DOM if present.
    pub fn extract_title(&self) -> Option<String> {
        let title_tag = LocalName::from("title");
        let count = self.len();
        for i in 0..count {
            let is_title = {
                let nodes = self.nodes.borrow();
                nodes
                    .get(i)
                    .and_then(|n| n.tag_name())
                    .map_or(false, |t| t == &title_tag)
            };
            if is_title {
                let text = self.get_text_content(i as u32);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }

    /// Serialize a DOM node and its descendants into valid XML/HTML markup (e.g. for SVG).
    pub fn serialize_node_xml(&self, node_id: u32) -> String {
        let mut out = String::new();
        self.serialize_node_xml_inner(node_id, &mut out);
        out
    }

    fn serialize_node_xml_inner(&self, node_id: u32, out: &mut String) {
        let nodes = self.nodes.borrow();
        let Some(node) = nodes.get(node_id as usize) else {
            return;
        };

        match &node.node_type {
            DomNodeType::Element { name, .. } => {
                let tag = name.to_string();
                out.push('<');
                out.push_str(&tag);
                for (k, v) in &node.attrs {
                    out.push(' ');
                    out.push_str(k.as_ref());
                    out.push_str("=\"");
                    // Escape basic XML chars
                    for ch in v.chars() {
                        match ch {
                            '&' => out.push_str("&amp;"),
                            '<' => out.push_str("&lt;"),
                            '>' => out.push_str("&gt;"),
                            '"' => out.push_str("&quot;"),
                            _ => out.push(ch),
                        }
                    }
                    out.push('"');
                }
                if node.children.is_empty() {
                    out.push_str(" />");
                } else {
                    out.push('>');
                    let child_ids = node.children.clone();
                    drop(nodes); // Drop borrow before recursion
                    for cid in child_ids {
                        self.serialize_node_xml_inner(cid.index() as u32, out);
                    }
                    out.push_str("</");
                    out.push_str(&tag);
                    out.push('>');
                }
            }
            DomNodeType::Text(text) => {
                for ch in text.chars() {
                    match ch {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        '>' => out.push_str("&gt;"),
                        _ => out.push(ch),
                    }
                }
            }
            _ => {}
        }
    }
}

// ------ Parser ------

/// HTML5 parser implementing \[TreeSink\].
pub struct DomParser {
    arena: DomArena,
    quirks_mode: RefCell<QuirksMode>,
}

impl DomParser {
    pub fn new() -> Self {
        Self {
            arena: DomArena::new(),
            quirks_mode: RefCell::new(QuirksMode::NoQuirks),
        }
    }

    pub fn into_arena(self) -> DomArena {
        self.arena
    }

    /// Get an element name by looking up the node in the arena.
    fn elem_name_from_arena(&self, id: NodeId) -> OwnedElemName {
        let nodes = self.arena.nodes.borrow();
        let node = &nodes[id.index()];
        match &node.node_type {
            DomNodeType::Element { name, namespace } => OwnedElemName {
                ns: namespace.clone(),
                local: name.clone(),
            },
            _ => OwnedElemName {
                ns: Namespace::from(""),
                local: LocalName::from(""),
            },
        }
    }
}

impl TreeSink for DomParser {
    type Handle = DomHandle;
    type Output = DomArena;
    type ElemName<'a> = OwnedElemName;

    fn finish(self) -> DomArena {
        self.arena
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        log::warn!("HTML parse error: {}", msg);
    }

    fn get_document(&self) -> DomHandle {
        DomHandle(NodeId::DOCUMENT)
    }

    fn elem_name<'a>(&'a self, target: &'a DomHandle) -> OwnedElemName {
        self.elem_name_from_arena(target.0)
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        _flags: ElementFlags,
    ) -> DomHandle {
        let mut attr_map = FxHashMap::default();
        for a in attrs {
            attr_map.insert(a.name.local.clone(), a.value.to_string());
        }

        let node = DomNode {
            node_type: DomNodeType::Element {
                name: name.local,
                namespace: name.ns,
            },
            attrs: attr_map,
            children: Vec::new(),
            parent: None,
            template_content: None,
        };
        DomHandle(self.arena.alloc(node))
    }

    fn create_comment(&self, text: StrTendril) -> DomHandle {
        let node = DomNode {
            node_type: DomNodeType::Comment(text.to_string()),
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: None,
            template_content: None,
        };
        DomHandle(self.arena.alloc(node))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> DomHandle {
        // HTML doesn't use PIs; create a no-op comment.
        let node = DomNode {
            node_type: DomNodeType::Comment(String::new()),
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: None,
            template_content: None,
        };
        DomHandle(self.arena.alloc(node))
    }

    fn append(&self, parent: &DomHandle, child: NodeOrText<DomHandle>) {
        match child {
            NodeOrText::AppendNode(h) => {
                self.arena.with_node_mut(h.0, |child_node| {
                    child_node.parent = Some(parent.0);
                });
                self.arena.with_node_mut(parent.0, |parent_node| {
                    parent_node.children.push(h.0);
                });
            }
            NodeOrText::AppendText(t) => {
                let text_id = self.arena.alloc(DomNode {
                    node_type: DomNodeType::Text(t.to_string()),
                    attrs: FxHashMap::default(),
                    children: Vec::new(),
                    parent: Some(parent.0),
                    template_content: None,
                });
                self.arena.with_node_mut(parent.0, |p| {
                    p.children.push(text_id);
                });
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &DomHandle,
        _prev_element: &DomHandle,
        child: NodeOrText<DomHandle>,
    ) {
        // Foster parenting: if element has no parent, append to element's parent.
        // Simplified: always append to element.
        self.append(element, child);
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let doctype_id = self.arena.alloc(DomNode {
            node_type: DomNodeType::Doctype {
                name: name.to_string(),
                public_id: public_id.to_string(),
                system_id: system_id.to_string(),
            },
            attrs: FxHashMap::default(),
            children: Vec::new(),
            parent: None,
            template_content: None,
        });
        self.arena.with_node_mut(NodeId::DOCUMENT, |doc| {
            doc.children.push(doctype_id);
        });
    }

    fn get_template_contents(&self, target: &DomHandle) -> DomHandle {
        self.arena
            .with_node(target.0, |n| n.template_content)
            .flatten()
            .map(DomHandle)
            .unwrap_or(DomHandle(NodeId::DOCUMENT))
    }

    fn same_node(&self, a: &DomHandle, b: &DomHandle) -> bool {
        a == b
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        *self.quirks_mode.borrow_mut() = mode;
    }

    fn append_before_sibling(&self, sibling: &DomHandle, new_node: NodeOrText<DomHandle>) {
        // Find sibling's parent and insert before sibling.
        let (parent_id, idx) = {
            let nodes = self.arena.nodes.borrow();
            let sib = &nodes[sibling.0.index()];
            let parent = sib.parent.expect("sibling has no parent");
            let parent_node = &nodes[parent.index()];
            let idx = parent_node
                .children
                .iter()
                .position(|&cid| cid == sibling.0)
                .expect("sibling not in parent's children");
            (parent, idx)
        };

        match new_node {
            NodeOrText::AppendNode(h) => {
                self.arena.with_node_mut(h.0, |n| {
                    n.parent = Some(parent_id);
                });
                self.arena.with_node_mut(parent_id, |p| {
                    p.children.insert(idx, h.0);
                });
            }
            NodeOrText::AppendText(t) => {
                let text_id = self.arena.alloc(DomNode {
                    node_type: DomNodeType::Text(t.to_string()),
                    attrs: FxHashMap::default(),
                    children: Vec::new(),
                    parent: Some(parent_id),
                    template_content: None,
                });
                self.arena.with_node_mut(parent_id, |p| {
                    p.children.insert(idx, text_id);
                });
            }
        }
    }

    fn add_attrs_if_missing(&self, target: &DomHandle, attrs: Vec<Attribute>) {
        self.arena.with_node_mut(target.0, |node| {
            for a in attrs {
                node.attrs
                    .entry(a.name.local)
                    .or_insert_with(|| a.value.to_string());
            }
        });
    }

    fn remove_from_parent(&self, target: &DomHandle) {
        let parent = match self.arena.with_node(target.0, |n| n.parent) {
            Some(Some(p)) => p,
            _ => return,
        };
        self.arena.with_node_mut(parent, |p| {
            p.children.retain(|&c| c != target.0);
        });
        self.arena.with_node_mut(target.0, |n| {
            n.parent = None;
        });
    }

    fn reparent_children(&self, node: &DomHandle, new_parent: &DomHandle) {
        let children: Vec<NodeId> = self
            .arena
            .with_node(node.0, |n| n.children.clone())
            .unwrap_or_default();

        for child_id in children {
            // Remove from old parent
            self.arena.with_node_mut(child_id, |c| {
                c.parent = Some(new_parent.0);
            });
            self.arena.with_node_mut(node.0, |n| {
                n.children.retain(|&c| c != child_id);
            });
            // Append to new parent
            self.arena.with_node_mut(new_parent.0, |p| {
                p.children.push(child_id);
            });
        }
    }
}

// ------ Tests ------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: get a child node by index from a parent.
    fn child(arena: &DomArena, parent: &DomNode, idx: usize) -> DomNode {
        arena
            .get(DomHandle(parent.children()[idx]))
            .expect("child exists")
    }

    /// Helper: navigate to the <body> element.
    /// html5ever auto-creates <head> and <body>, so html's children are [head, body].
    fn body_of(arena: &DomArena, doc: &DomNode) -> DomNode {
        let html = child(arena, doc, 0);
        child(arena, &html, 1) // index 1 is <body> (0 is <head>)
    }

    #[test]
    fn parse_returns_root() {
        let arena = parse_html("<html><body>Hello</body></html>");
        let doc = arena.get(arena.document_handle()).expect("document");
        assert_eq!(doc.children().len(), 1); // <html>
    }

    #[test]
    fn parse_body_has_children() {
        let arena = parse_html("<html><body><div>A</div><span>B</span></body></html>");
        let doc = arena.get(arena.document_handle()).unwrap();
        let body = body_of(&arena, &doc);
        assert_eq!(body.children().len(), 2); // <div>, <span>
    }

    #[test]
    fn parse_text_node() {
        let arena = parse_html("<div>Hello World</div>");
        let doc = arena.get(arena.document_handle()).unwrap();
        let body = body_of(&arena, &doc);
        let div = child(&arena, &body, 0);
        assert!(div.is_element());
        assert!(!div.children().is_empty()); // text "Hello World"
    }

    #[test]
    fn parse_attributes() {
        let arena = parse_html("<a href='https://example.com' class='link'>click</a>");
        let doc = arena.get(arena.document_handle()).unwrap();
        let body = body_of(&arena, &doc);
        let a = child(&arena, &body, 0);
        assert_eq!(a.get_attr("href"), Some("https://example.com"));
        assert_eq!(a.get_attr("class"), Some("link"));
    }

    #[test]
    fn parse_comment() {
        let arena = parse_html("<!-- this is a comment -->");
        // html5ever places comments at the document level for short documents
        let doc = arena.get(arena.document_handle()).unwrap();
        let found = doc.children().iter().any(|&cid| {
            arena
                .get(DomHandle(cid))
                .map(|n| n.is_comment())
                .unwrap_or(false)
        });
        assert!(found, "Expected comment as child of document");
    }

    #[test]
    fn parse_nested_elements() {
        let arena = parse_html("<div><ul><li>one</li><li>two</li></ul></div>");
        let doc = arena.get(arena.document_handle()).unwrap();
        assert!(doc.children().len() >= 1);
    }

    #[test]
    fn dom_node_set_attribute() {
        let mut node = DomNode::element("a");
        node.set_attribute("href", "https://example.com");
        assert_eq!(node.get_attr("href"), Some("https://example.com"));
    }
}
