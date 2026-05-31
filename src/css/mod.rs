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
            CSSColor::Named(_) => (0, 0, 0, 255), // fallback
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
pub fn compute_styles(_properties: FxHashMap<String, String>) -> FxHashMap<String, String> {
    // TODO: implement cascade, inheritance, computed values
    FxHashMap::default()
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
        assert_eq!(parse_color_value("#ff0000"), Some(CSSColor::Hex { r: 255, g: 0, b: 0 }));
    }

    #[test]
    fn parse_color_hex_short() {
        assert_eq!(parse_color_value("#f00"), Some(CSSColor::Hex { r: 255, g: 0, b: 0 }));
    }

    #[test]
    fn parse_color_invalid() {
        assert!(parse_color_value("invalid").is_none());
    }
}
