/// Memory profiling module.
///
/// Estimates the heap memory consumption of key engine data structures
/// at runtime. Enabled via the `memprof` feature flag.
///
/// These are *estimates* — they use `size_of` for allocated metadata and
/// measure capacity/length of owned collections. They do not track GPU
/// memory, thread-local caches, or the wgpu device pool.
use std::mem::size_of;

use crate::css::ComputedValues;
use crate::html::{DomArena, DomNode};
use crate::layout::LayoutNode;
use crate::page::{CachedImage, Page};

/// Per-category memory breakdown.
#[derive(Debug, Default)]
pub struct MemoryProfile {
    pub dom_arena_bytes: usize,
    pub style_map_bytes: usize,
    pub layout_tree_bytes: usize,
    pub image_cache_bytes: usize,
    pub composite_buffer_bytes: usize,
    pub total_bytes: usize,
}

impl MemoryProfile {
    /// Format the profile as a human-readable summary (in KiB/MiB).
    pub fn summary(&self) -> String {
        format!(
            "Memory profile — DOM: {}, Styles: {}, Layout: {}, Images: {}, Composite: {}, Total: {}",
            Self::human_size(self.dom_arena_bytes),
            Self::human_size(self.style_map_bytes),
            Self::human_size(self.layout_tree_bytes),
            Self::human_size(self.image_cache_bytes),
            Self::human_size(self.composite_buffer_bytes),
            Self::human_size(self.total_bytes),
        )
    }

    fn human_size(bytes: usize) -> String {
        if bytes >= 1024 * 1024 {
            format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes >= 1024 {
            format!("{:.2} KiB", bytes as f64 / 1024.0)
        } else {
            format!("{} B", bytes)
        }
    }
}

// --------------------------------------------------------------------------
// Public API
// --------------------------------------------------------------------------

/// Estimate the memory used by a fully loaded `Page`.
///
/// Pass the composite buffer size (in bytes) so the profiler can include it.
pub fn profile_page(page: &Page, composite_buffer_bytes: usize) -> MemoryProfile {
    let dom = estimate_dom_arena(&page.arena);
    let styles = estimate_style_map(&page.styles);
    let layout = estimate_layout_tree(&page.layout_root);
    let images = estimate_image_cache(&page.image_cache);

    let total = dom + styles + layout + images + composite_buffer_bytes;

    MemoryProfile {
        dom_arena_bytes: dom,
        style_map_bytes: styles,
        layout_tree_bytes: layout,
        image_cache_bytes: images,
        composite_buffer_bytes,
        total_bytes: total,
    }
}

// --------------------------------------------------------------------------
// DOM arena estimation
// --------------------------------------------------------------------------

/// Estimate the memory footprint of the DOM arena.
///
/// Counts each `DomNode`'s struct size plus the owned data in `attrs` and
/// text content stored inside `node_type`.
fn estimate_dom_arena(arena: &DomArena) -> usize {
    let nodes = arena.nodes.borrow();

    // Base: Vec allocation overhead + capacity * size_of<DomNode>
    let mut total: usize = nodes.capacity() * size_of::<DomNode>();

    for node in nodes.iter() {
        // Attributes: FxHashMap entry overhead + key (LocalName wraps ArcStr) + value (String)
        // LocalName holds an ArcStr, so we count the string content via the Display/Clone path.
        // We approximate: each attr entry ~ size_of<LocalName> + size_of<String> + hashbrown entry overhead
        total += node.attrs.len()
            * (size_of::<std::collections::hash_map::RandomState>() /* rough */
            + 32 /* ArcStr header average */
            + 32/* String header avg short string */)
            + node.attrs.capacity() * size_of::<usize>();

        // Children vec
        total += node.children.capacity() * size_of::<crate::html::NodeId>();

        // Text content inside DomNodeType::Text or ::Comment
        if let crate::html::DomNodeType::Text(ref s) = node.node_type {
            total += s.len();
        } else if let crate::html::DomNodeType::Comment(ref s) = node.node_type {
            total += s.len();
        } else if let crate::html::DomNodeType::Doctype {
            ref name,
            ref public_id,
            ref system_id,
        } = node.node_type
        {
            total += name.len() + public_id.len() + system_id.len();
        }
    }

    total
}

// --------------------------------------------------------------------------
// Style map estimation
// --------------------------------------------------------------------------

/// Estimate the memory used by the computed styles hash map.
fn estimate_style_map(styles: &rustc_hash::FxHashMap<u32, ComputedValues>) -> usize {
    // HashMap bucket overhead + entries
    let mut total: usize = styles.capacity() * size_of::<usize>();

    for (_, cv) in styles.iter() {
        total += estimate_computed_values(cv);
    }

    total
}

/// Estimate the heap cost of a single `ComputedValues`.
fn estimate_computed_values(cv: &ComputedValues) -> usize {
    let mut total: usize = size_of::<ComputedValues>();

    // String fields
    total += cv.font_family.len();

    // Vec<GridTrack> fields
    total += cv.grid_template_columns.capacity() * size_of::<crate::css::GridTrack>();
    total += cv.grid_template_rows.capacity() * size_of::<crate::css::GridTrack>();

    total
}

// --------------------------------------------------------------------------
// Layout tree estimation
// --------------------------------------------------------------------------

/// Recursively estimate the memory used by the layout tree rooted at `node`.
fn estimate_layout_tree(node: &LayoutNode) -> usize {
    let mut total: usize = size_of::<LayoutNode>();

    // Owned text content
    if let Some(ref text) = node.text {
        total += text.len();
    }

    // image_src string
    if let Some(ref src) = node.image_src {
        total += src.len();
    }

    // font_family string
    total += node.font_family.len();

    // children vec capacity
    total += node.children.capacity() * size_of::<LayoutNode>();

    // absolute_children vec capacity
    total += node.absolute_children.capacity() * size_of::<LayoutNode>();

    // line_boxes
    if let Some(ref boxes) = node.line_boxes {
        total += estimate_line_boxes(boxes);
    }

    // grid_columns / grid_rows vec capacity (already counted as part of the struct,
    // but the Vec inline data lives inside LayoutNode — capacity is extra heap)
    total += node.grid_columns.capacity() * size_of::<crate::css::GridTrack>();
    total += node.grid_rows.capacity() * size_of::<crate::css::GridTrack>();

    // Recurse
    for child in &node.children {
        total += estimate_layout_tree(child);
    }
    for abs_child in &node.absolute_children {
        total += estimate_layout_tree(abs_child);
    }

    total
}

/// Estimate the heap cost of a list of `LineBox` entries.
fn estimate_line_boxes(boxes: &[crate::layout::LineBox]) -> usize {
    // We receive a slice, so we only know the length, not capacity.
    let mut total: usize = std::mem::size_of_val(boxes);

    for lb in boxes {
        total += lb.boxes.capacity() * size_of::<crate::layout::InlineBox>();
        for ib in &lb.boxes {
            if let crate::layout::InlineBox::Text { text, .. } = ib {
                total += text.len();
            }
            // Element and Whitespace variants carry no owned heap data beyond the struct
        }
    }

    total
}

// --------------------------------------------------------------------------
// Image cache estimation
// --------------------------------------------------------------------------

/// Estimate the memory used by the cached decoded images.
fn estimate_image_cache(cache: &rustc_hash::FxHashMap<String, CachedImage>) -> usize {
    let mut total: usize = cache.capacity() * size_of::<usize>();

    for (key, img) in cache.iter() {
        // Key string
        total += key.len();
        // RGBA pixel data
        total += img.rgba.len();
    }

    total
}

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_empty_page() {
        let page = crate::page::Page::new("<html><body></body></html>", "", 800.0, 600.0);
        let profile = profile_page(&page, 0);

        assert!(profile.dom_arena_bytes > 0, "DOM arena should use memory");
        assert!(
            profile.layout_tree_bytes > 0,
            "Layout tree should use memory"
        );
        assert_eq!(profile.image_cache_bytes, 0, "Empty cache should be 0");
        assert_eq!(profile.composite_buffer_bytes, 0);
        assert!(profile.total_bytes > 0);
    }

    #[test]
    fn profile_with_image_cache() {
        let mut page = crate::page::Page::new("<html><body></body></html>", "", 800.0, 600.0);
        page.image_cache.insert(
            "https://example.com/test.png".to_string(),
            CachedImage {
                rgba: vec![255u8; 640 * 480 * 4], // 640x480 image
                width: 640,
                height: 480,
            },
        );

        let profile = profile_page(&page, 0);
        // 640*480*4 = 1_228_800 bytes for the image data
        assert!(
            profile.image_cache_bytes > 1_000_000,
            "Image cache should account for pixel data, got {}",
            profile.image_cache_bytes
        );
    }

    #[test]
    fn composite_buffer_included() {
        let page = crate::page::Page::new("<html><body></body></html>", "", 800.0, 600.0);
        let comp_size = 1280 * 800 * 4; // ~4 MB RGBA buffer
        let profile = profile_page(&page, comp_size);

        assert_eq!(profile.composite_buffer_bytes, comp_size);
        assert!(profile.total_bytes >= comp_size);
    }

    #[test]
    fn summary_contains_module_names() {
        let profile = MemoryProfile {
            dom_arena_bytes: 1024,
            style_map_bytes: 2048,
            layout_tree_bytes: 4096,
            image_cache_bytes: 1024 * 1024,
            composite_buffer_bytes: 1024 * 1024 * 2,
            total_bytes: 0,
        };
        let s = profile.summary();
        assert!(s.contains("DOM"));
        assert!(s.contains("Styles"));
        assert!(s.contains("Layout"));
        assert!(s.contains("Images"));
        assert!(s.contains("Composite"));
    }

    #[test]
    fn human_size_kilobyte() {
        assert!(MemoryProfile::human_size(1024).contains("KiB"));
    }

    #[test]
    fn human_size_megabyte() {
        assert!(MemoryProfile::human_size(1_048_576).contains("MiB"));
    }

    #[test]
    fn human_size_byte() {
        assert!(MemoryProfile::human_size(500).contains("B"));
    }
}
