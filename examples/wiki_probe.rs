//! Measure the benchmark page.
//!
//! `cargo run --example wiki_fetch -- <dir>` first, then this against the same
//! directory. Fetching once and measuring many times keeps a change's effect
//! separate from the page changing under us.

use mistilteinn::layout::LayoutNode;
use mistilteinn::page::Page;

fn walk<'a>(node: &'a LayoutNode, out: &mut Vec<&'a LayoutNode>) {
    out.push(node);
    for child in &node.children {
        walk(child, out);
    }
    for child in &node.absolute_children {
        walk(child, out);
    }
}

/// The tag and class of the element a box came from, for reading the report.
fn describe(page: &Page, node: &LayoutNode) -> String {
    node.dom_node_id
        .and_then(|id| {
            page.arena
                .get(mistilteinn::html::DomHandle(
                    mistilteinn::html::NodeId::from_raw(id),
                ))
                .map(|n| {
                    let class = n.get_attr("class").unwrap_or("");
                    format!(
                        "<{}> {}",
                        n.tag_name().map(|t| t.to_string()).unwrap_or_default(),
                        &class[..class.len().min(48)]
                    )
                })
        })
        .unwrap_or_default()
}

fn main() {
    let dir = std::env::args().nth(1).expect("fixture dir");
    let html = std::fs::read_to_string(format!("{dir}/wiki.html")).unwrap();
    let css = std::fs::read_to_string(format!("{dir}/wiki.css")).unwrap();
    let page = Page::new(&html, &css, 1280.0, 800.0);
    let mut nodes = Vec::new();
    walk(&page.layout_root, &mut nodes);
    let runs = mistilteinn::layout::collect_text_nodes(&page.layout_root);

    println!("== totals ==");
    println!("boxes {}, text runs {}", nodes.len(), runs.len());
    println!(
        "styled elements {}, of them display:none {}",
        page.styles.len(),
        page.styles
            .values()
            .filter(|s| s.display == mistilteinn::css::DisplayType::None)
            .count()
    );

    println!("\n== list markers ==");
    println!(
        "items {}, bullets painted {}, lists asking for none {}",
        page.styles.values().filter(|s| s.list_item).count(),
        runs.iter()
            .filter(|t| matches!(t.text.as_str(), "\u{2022}" | "\u{25e6}" | "\u{25aa}"))
            .count(),
        page.styles
            .values()
            .filter(|s| s.list_style_type == Some(mistilteinn::css::ListStyleType::None))
            .count()
    );

    println!("\n== masked icons ==");
    println!(
        "styles carrying a mask {}, boxes laid out with one {}",
        page.styles
            .values()
            .filter(|s| s.mask_image.is_some())
            .count(),
        nodes.iter().filter(|n| n.mask_image.is_some()).count()
    );

    println!("\n== text that lands on top of other text ==");
    let mut sorted = runs.clone();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    let mut overlaps = 0;
    for pair in sorted.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if (a.y - b.y).abs() < 1.0 && b.x < a.x + a.width - 0.5 {
            overlaps += 1;
            if overlaps <= 5 {
                println!(
                    "  y={:7.1}: {:?} ends at {:.1}, {:?} starts at {:.1}",
                    a.y,
                    &a.text[..a.text.len().min(12)],
                    a.x + a.width,
                    &b.text[..b.text.len().min(12)],
                    b.x
                );
            }
        }
    }
    println!("  {overlaps} of {} runs", runs.len());

    println!("\n== boxes shorter than what is inside them ==");
    let mut spilling = 0;
    for node in &nodes {
        if node.children.is_empty() || node.rect.height <= 0.0 {
            continue;
        }
        let bottom = node
            .children
            .iter()
            .map(|c| c.rect.y + c.rect.height)
            .fold(f32::MIN, f32::max);
        let spill = bottom - (node.rect.y + node.rect.height);
        if spill > 2.0 {
            spilling += 1;
            if spilling <= 8 {
                println!(
                    "  {:?} {:7.1}x{:6.1} spills {:5.1} {}",
                    node.display,
                    node.rect.width,
                    node.rect.height,
                    spill,
                    describe(&page, node)
                );
            }
        }
    }
    println!("  {spilling} boxes");

    println!("\n== pictures, against what the markup asked for ==");
    for node in nodes.iter().filter(|n| n.image_src.is_some()).take(8) {
        let src = node.image_src.as_deref().unwrap_or("");
        let name = src
            .split('/')
            .next_back()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("");
        let asked = node.dom_node_id.and_then(|id| {
            page.arena
                .get(mistilteinn::html::DomHandle(
                    mistilteinn::html::NodeId::from_raw(id),
                ))
                .map(|n| {
                    format!(
                        "{}x{}",
                        n.get_attr("width").unwrap_or("-"),
                        n.get_attr("height").unwrap_or("-")
                    )
                })
        });
        println!(
            "  laid out {:6.1}x{:6.1}  markup {:9}  {}",
            node.rect.width,
            node.rect.height,
            asked.unwrap_or_else(|| "-".into()),
            &name[..name.len().min(40)]
        );
    }

    println!("\n== sticky boxes ==");
    for node in nodes
        .iter()
        .filter(|n| n.position == mistilteinn::css::PositionType::Sticky)
    {
        println!(
            "  {:6.1}x{:5.1} at {:.1},{:.1} insets={:?} {}",
            node.rect.width,
            node.rect.height,
            node.rect.x,
            node.rect.y,
            node.offsets,
            describe(&page, node)
        );
    }
}
