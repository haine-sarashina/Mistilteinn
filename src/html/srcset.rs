//! Choosing one image out of a `srcset`.
//!
//! `srcset` offers the same picture at several sizes and lets the browser pick.
//! Taking the first candidate — which is what this engine did — means taking
//! whichever the author happened to list first: usually the smallest, so a wide
//! image renders from a thumbnail, or the largest, so a thumbnail costs a
//! full-size download.

/// One entry of a `srcset` list.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub url: String,
    /// The descriptor that says what this candidate is for.
    pub descriptor: Descriptor,
}

/// What a candidate is offered for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Descriptor {
    /// `480w` — the image is this many pixels wide.
    Width(f32),
    /// `2x` — the image is for a display with this device pixel ratio.
    Density(f32),
}

/// Parse a `srcset` attribute into its candidates.
///
/// Entries are comma-separated, each a URL followed by an optional descriptor.
/// A URL may itself contain a comma (a query string, a data URI), so the split
/// is on a comma *followed by whitespace and a URL* rather than on every comma;
/// entries in the wild are written with a space after the comma for exactly
/// this reason. An entry with no descriptor is `1x`, per the spec.
pub fn parse(attribute: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    for entry in split_entries(attribute) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split_whitespace();
        let Some(url) = parts.next() else { continue };
        let descriptor = match parts.next() {
            Some(token) => match parse_descriptor(token) {
                Some(descriptor) => descriptor,
                // An unreadable descriptor makes the candidate unusable rather
                // than a default-density one: guessing could pick a 2000px
                // image for a thumbnail slot.
                None => continue,
            },
            None => Descriptor::Density(1.0),
        };
        candidates.push(Candidate {
            url: url.to_string(),
            descriptor,
        });
    }

    candidates
}

/// Split on the commas that separate entries, not on commas inside a URL.
fn split_entries(attribute: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0usize;
    let bytes = attribute.as_bytes();

    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b',' {
            continue;
        }
        // A comma inside a URL is not followed by whitespace; one that
        // separates entries is (or ends the list).
        let is_separator = attribute[index + 1..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace());
        if is_separator {
            entries.push(&attribute[start..index]);
            start = index + 1;
        }
    }
    entries.push(&attribute[start..]);
    entries
}

fn parse_descriptor(token: &str) -> Option<Descriptor> {
    if let Some(number) = token.strip_suffix('w') {
        let width: f32 = number.parse().ok()?;
        return (width > 0.0).then_some(Descriptor::Width(width));
    }
    if let Some(number) = token.strip_suffix('x') {
        let density: f32 = number.parse().ok()?;
        return (density > 0.0).then_some(Descriptor::Density(density));
    }
    None
}

/// Pick the candidate to load for a slot `display_width` CSS pixels wide on a
/// display of `device_pixel_ratio`.
///
/// Width candidates are judged against the slot: the narrowest one that still
/// covers it, falling back to the widest available when none does — an image
/// scaled down is right, one scaled up is blurry. Density candidates are judged
/// against the display the same way. Mixed lists prefer width candidates, since
/// those carry the more specific information.
pub fn select(
    candidates: &[Candidate],
    display_width: f32,
    device_pixel_ratio: f32,
) -> Option<&str> {
    if candidates.is_empty() {
        return None;
    }

    let needed = display_width * device_pixel_ratio;
    let widths: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| matches!(c.descriptor, Descriptor::Width(_)))
        .collect();

    if !widths.is_empty() && display_width > 0.0 {
        let width_of = |c: &Candidate| match c.descriptor {
            Descriptor::Width(w) => w,
            Descriptor::Density(_) => 0.0,
        };
        let covering = widths
            .iter()
            .filter(|c| width_of(c) >= needed)
            .min_by(|a, b| width_of(a).total_cmp(&width_of(b)));
        let chosen = covering.or_else(|| {
            widths
                .iter()
                .max_by(|a, b| width_of(a).total_cmp(&width_of(b)))
        })?;
        return Some(&chosen.url);
    }

    // No width descriptors, or no slot width to judge them by: fall back to
    // density, where 1x is what this renderer draws at.
    let density_of = |c: &Candidate| match c.descriptor {
        Descriptor::Density(d) => d,
        // A width candidate in a density list is treated as the largest
        // possible density so it is only chosen when nothing else fits.
        Descriptor::Width(_) => f32::INFINITY,
    };
    let covering = candidates
        .iter()
        .filter(|c| density_of(c) >= device_pixel_ratio)
        .min_by(|a, b| density_of(a).total_cmp(&density_of(b)));
    let chosen = covering.or_else(|| {
        candidates
            .iter()
            .max_by(|a, b| density_of(a).total_cmp(&density_of(b)))
    })?;
    Some(&chosen.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_of_widths_is_parsed_with_its_descriptors() {
        let parsed = parse("small.png 480w, large.png 1200w");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].url, "small.png");
        assert_eq!(parsed[0].descriptor, Descriptor::Width(480.0));
        assert_eq!(parsed[1].descriptor, Descriptor::Width(1200.0));
    }

    #[test]
    fn a_candidate_with_no_descriptor_is_a_single_density_one() {
        let parsed = parse("plain.png");
        assert_eq!(parsed[0].descriptor, Descriptor::Density(1.0));
    }

    #[test]
    fn a_comma_inside_a_url_does_not_split_the_entry() {
        // Query strings and data URIs contain commas; splitting on every one
        // produces two broken URLs out of a working one.
        let parsed = parse("https://cdn.example/img?w=1,h=2 800w, other.png 400w");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].url, "https://cdn.example/img?w=1,h=2");
        assert_eq!(parsed[1].url, "other.png");
    }

    #[test]
    fn an_unreadable_descriptor_drops_that_candidate() {
        let parsed = parse("good.png 400w, weird.png 12q");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].url, "good.png");
    }

    #[test]
    fn the_narrowest_candidate_that_covers_the_slot_wins() {
        let candidates = parse("a.png 400w, b.png 800w, c.png 1600w");
        assert_eq!(select(&candidates, 600.0, 1.0), Some("b.png"));
        assert_eq!(select(&candidates, 400.0, 1.0), Some("a.png"));
    }

    #[test]
    fn the_widest_candidate_is_used_when_none_covers_the_slot() {
        // Scaling an image down looks right; scaling one up does not.
        let candidates = parse("a.png 400w, b.png 800w");
        assert_eq!(select(&candidates, 2000.0, 1.0), Some("b.png"));
    }

    #[test]
    fn a_dense_display_asks_for_more_pixels_than_the_slot_is_wide() {
        let candidates = parse("a.png 400w, b.png 800w, c.png 1600w");
        assert_eq!(select(&candidates, 400.0, 2.0), Some("b.png"));
    }

    #[test]
    fn density_candidates_are_picked_by_the_display() {
        let candidates = parse("one.png, two.png 2x, three.png 3x");
        assert_eq!(select(&candidates, 0.0, 1.0), Some("one.png"));
        assert_eq!(select(&candidates, 0.0, 2.0), Some("two.png"));
        assert_eq!(
            select(&candidates, 0.0, 4.0),
            Some("three.png"),
            "nothing is dense enough, so the densest available is used"
        );
    }

    #[test]
    fn a_width_list_falls_back_to_density_when_the_slot_size_is_unknown() {
        // An image with no size yet has no slot to measure against; picking the
        // smallest is what a browser does before layout knows better.
        let candidates = parse("a.png 400w, b.png 1600w");
        assert!(select(&candidates, 0.0, 1.0).is_some());
    }

    #[test]
    fn an_empty_or_blank_attribute_selects_nothing() {
        assert!(select(&parse(""), 100.0, 1.0).is_none());
        assert!(select(&parse("   "), 100.0, 1.0).is_none());
    }

    #[test]
    fn extra_whitespace_and_trailing_commas_are_tolerated() {
        let parsed = parse("  a.png   400w ,  b.png 800w ,  ");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].url, "b.png");
    }
}
