//! The `filter` property.
//!
//! A filter is a paint-time effect: the element and its descendants are drawn
//! into a buffer of their own, the buffer is put through the listed functions,
//! and the result is composited back. Nothing here changes layout — a blurred
//! box occupies exactly the space it would have unblurred, it just spills paint
//! outside it.

use super::{LengthContext, parse_angle, parse_color_value, parse_length_ctx};

/// One entry of a `filter` list.
///
/// The colour-matrix functions all carry their amount as a fraction, so `100%`
/// and `1` arrive here as the same number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterFn {
    /// Standard deviation of the Gaussian, in pixels.
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    /// Rotation around the colour wheel, in radians.
    HueRotate(f32),
    DropShadow {
        dx: f32,
        dy: f32,
        /// Standard deviation of the shadow's Gaussian, in pixels.
        blur: f32,
        color: [u8; 4],
    },
}

/// The computed `filter` property.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    functions: Vec<FilterFn>,
}

impl Filter {
    pub fn is_none(&self) -> bool {
        self.functions.is_empty()
    }

    pub fn functions(&self) -> &[FilterFn] {
        &self.functions
    }

    /// Build a filter from a list of functions, for animation and for tests.
    pub fn from_functions(functions: Vec<FilterFn>) -> Self {
        Self { functions }
    }

    /// How far, in pixels, the filtered result can reach outside the box.
    ///
    /// A blur spreads paint in every direction and a drop shadow throws it to
    /// one side; the painter needs to know how much room to give the buffer it
    /// draws into, or the effect is cut off at the box's own edge.
    pub fn outset(&self) -> f32 {
        let mut outset: f32 = 0.0;
        for function in &self.functions {
            let reach = match *function {
                // Three box-blur passes cover about three standard deviations.
                FilterFn::Blur(sigma) => sigma * 3.0,
                FilterFn::DropShadow { dx, dy, blur, .. } => dx.abs().max(dy.abs()) + blur * 3.0,
                _ => 0.0,
            };
            outset = outset.max(reach);
        }
        outset.ceil()
    }

    /// Parse a `filter` value.
    ///
    /// An unrecognised function is dropped rather than voiding the whole list:
    /// a page asking for `filter: blur(2px) url(#thing)` is better off with the
    /// blur than with nothing.
    pub fn parse(value: &str, ctx: LengthContext) -> Self {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("none") {
            return Self::default();
        }

        let mut functions = Vec::new();
        let mut rest = value;
        while let Some(open) = rest.find('(') {
            let name = rest[..open].trim().to_ascii_lowercase();
            // A `drop-shadow` colour can itself be `rgb(...)`, so the closing
            // parenthesis is the one that balances this opening one.
            let Some(close) = matching_paren(&rest[open..]) else {
                break;
            };
            let args = rest[open + 1..open + close].trim().to_string();
            rest = &rest[open + close + 1..];

            let parsed = match name.as_str() {
                "blur" => parse_length_ctx(&args, ctx).map(FilterFn::Blur),
                "brightness" => parse_amount(&args).map(FilterFn::Brightness),
                "contrast" => parse_amount(&args).map(FilterFn::Contrast),
                "grayscale" | "greyscale" => parse_amount(&args).map(FilterFn::Grayscale),
                "invert" => parse_amount(&args).map(FilterFn::Invert),
                "opacity" => parse_amount(&args).map(FilterFn::Opacity),
                "saturate" => parse_amount(&args).map(FilterFn::Saturate),
                "sepia" => parse_amount(&args).map(FilterFn::Sepia),
                "hue-rotate" => parse_angle(&args).map(FilterFn::HueRotate),
                "drop-shadow" => parse_drop_shadow(&args, ctx),
                _ => None,
            };
            functions.extend(parsed);
        }

        Self { functions }
    }
}

/// `drop-shadow(<x> <y> <blur>? <color>?)`, in either order of colour and
/// lengths — both `drop-shadow(red 2px 2px)` and `drop-shadow(2px 2px red)`
/// are written in the wild.
fn parse_drop_shadow(args: &str, ctx: LengthContext) -> Option<FilterFn> {
    let mut lengths = Vec::new();
    let mut color = None;

    for token in split_outside_parens(args) {
        if let Some(px) = parse_length_ctx(&token, ctx) {
            lengths.push(px);
        } else if let Some(parsed) = parse_color_value(&token) {
            let (r, g, b, a) = parsed.to_rgba();
            color = Some([r, g, b, a]);
        }
    }

    if lengths.len() < 2 {
        return None;
    }
    Some(FilterFn::DropShadow {
        dx: lengths[0],
        dy: lengths[1],
        blur: lengths.get(2).copied().unwrap_or(0.0).max(0.0),
        // CSS says the shadow takes `currentColor`; black is the colour text
        // has unless the page says otherwise, which is the common case.
        color: color.unwrap_or([0, 0, 0, 255]),
    })
}

/// A filter amount: a bare number, or a percentage of one.
fn parse_amount(token: &str) -> Option<f32> {
    let token = token.trim();
    if let Some(percent) = token.strip_suffix('%') {
        return percent.trim().parse::<f32>().ok().map(|p| p / 100.0);
    }
    token.parse::<f32>().ok()
}

/// The index, within `s` (which starts at `(`), of the `)` that closes it.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split on whitespace, keeping anything inside parentheses together.
fn split_outside_parens(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> Vec<FilterFn> {
        Filter::parse(value, LengthContext::default())
            .functions()
            .to_vec()
    }

    fn outset(value: &str) -> f32 {
        Filter::parse(value, LengthContext::default()).outset()
    }

    #[test]
    fn none_and_empty_produce_no_functions() {
        assert!(Filter::parse("none", LengthContext::default()).is_none());
        assert!(Filter::parse("  ", LengthContext::default()).is_none());
    }

    #[test]
    fn percentages_and_bare_numbers_mean_the_same_thing() {
        assert_eq!(parse("grayscale(50%)"), vec![FilterFn::Grayscale(0.5)]);
        assert_eq!(parse("grayscale(0.5)"), vec![FilterFn::Grayscale(0.5)]);
    }

    #[test]
    fn a_list_keeps_its_order() {
        assert_eq!(
            parse("blur(2px) brightness(1.2) invert(100%)"),
            vec![
                FilterFn::Blur(2.0),
                FilterFn::Brightness(1.2),
                FilterFn::Invert(1.0),
            ]
        );
    }

    #[test]
    fn hue_rotate_reads_an_angle() {
        match parse("hue-rotate(180deg)")[0] {
            FilterFn::HueRotate(radians) => {
                assert!((radians - std::f32::consts::PI).abs() < 1e-4)
            }
            other => panic!("expected a rotation, got {other:?}"),
        }
    }

    #[test]
    fn drop_shadow_reads_offsets_blur_and_colour() {
        assert_eq!(
            parse("drop-shadow(2px 4px 6px #ff0000)"),
            vec![FilterFn::DropShadow {
                dx: 2.0,
                dy: 4.0,
                blur: 6.0,
                color: [255, 0, 0, 255],
            }]
        );
    }

    #[test]
    fn drop_shadow_defaults_to_a_sharp_black_shadow() {
        assert_eq!(
            parse("drop-shadow(3px 3px)"),
            vec![FilterFn::DropShadow {
                dx: 3.0,
                dy: 3.0,
                blur: 0.0,
                color: [0, 0, 0, 255],
            }]
        );
    }

    #[test]
    fn a_colour_written_as_a_function_does_not_end_the_argument_list() {
        assert_eq!(
            parse("drop-shadow(1px 2px 3px rgb(10, 20, 30))"),
            vec![FilterFn::DropShadow {
                dx: 1.0,
                dy: 2.0,
                blur: 3.0,
                color: [10, 20, 30, 255],
            }]
        );
    }

    #[test]
    fn an_unknown_function_does_not_take_the_rest_of_the_list_with_it() {
        assert_eq!(
            parse("url(#svg-thing) blur(3px)"),
            vec![FilterFn::Blur(3.0)]
        );
    }

    #[test]
    fn outset_covers_the_reach_of_blur_and_shadow() {
        assert_eq!(outset("blur(2px)"), 6.0);
        assert_eq!(outset("drop-shadow(10px 0 2px black)"), 16.0);
        assert_eq!(outset("grayscale(1)"), 0.0);
    }
}
