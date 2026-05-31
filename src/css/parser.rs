use rustc_hash::FxHashMap;

/// A parsed CSS declaration (property: value).
#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: String,
    pub value: String,
}

/// Parses a simple CSS declaration list into property-value pairs.
///
/// Example: `"color: red; margin: 10px"` → `[(color, red), (margin, 10px)]`
///
/// Note: This is a simplified parser. Full CSS parsing (at-rules, selectors,
/// media queries, etc.) will be implemented incrementally.
pub fn parse_declarations(source: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();

    for line in source.split(';') {
        let parts: Vec<&str> = line.splitn(2, ':').map(|s| s.trim()).collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            declarations.push(Declaration {
                property: parts[0].to_string(),
                value: parts[1].to_string(),
            });
        }
    }

    declarations
}

/// Parses a CSS color value (simplified — uses named color map).
pub fn parse_color_value(color_str: &str) -> Option<(u8, u8, u8)> {
    let color = color_str.trim().to_lowercase();

    // Named colors (subset)
    let named_colors: FxHashMap<&str, (u8, u8, u8)> = [
        ("red", (255, 0, 0)),
        ("green", (0, 128, 0)),
        ("blue", (0, 0, 255)),
        ("white", (255, 255, 255)),
        ("black", (0, 0, 0)),
    ].into_iter().collect();

    if let Some(&rgb) = named_colors.get(color.as_str()) {
        return Some(rgb);
    }

    // Hex colors: #RRGGBB
    if color.starts_with('#') && color.len() == 7 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&color[1..3], 16),
            u8::from_str_radix(&color[3..5], 16),
            u8::from_str_radix(&color[5..7], 16),
        ) {
            return Some((r, g, b));
        }
    }

    None
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
    fn parse_color_red() {
        assert_eq!(parse_color_value("red"), Some((255, 0, 0)));
    }

    #[test]
    fn parse_color_hex() {
        assert_eq!(parse_color_value("#ff0000"), Some((255, 0, 0)));
    }

    #[test]
    fn parse_color_invalid() {
        assert!(parse_color_value("invalid").is_none());
    }
}
