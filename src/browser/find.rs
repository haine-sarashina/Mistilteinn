//! In-page search — the text side of the find bar.
//!
//! Locating the matches is separate from drawing them: this module answers
//! "where in this run's text does the query occur", and the caller turns each
//! answer into a highlight by measuring the text before it.

/// One occurrence of the query inside one text run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    /// Which run of the page the match is in.
    pub run: usize,
    /// Byte offsets of the match within that run's own text.
    pub start: usize,
    pub end: usize,
}

/// Every occurrence of `query` across `runs`, in document order.
///
/// Matching is case-insensitive and does not cross a run boundary: a phrase
/// split over two lines is not found, because the two halves are laid out in
/// different places and there is no single rectangle to highlight.
pub fn find_matches(runs: &[String], query: &str) -> Vec<TextMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let needle = lowercase(query);
    let mut out = Vec::new();

    for (run, text) in runs.iter().enumerate() {
        let (haystack, offsets) = lowercase_with_offsets(text);
        let mut from = 0usize;
        while let Some(found) = haystack[from..].find(&needle) {
            let lower_start = from + found;
            let lower_end = lower_start + needle.len();
            out.push(TextMatch {
                run,
                start: offsets[lower_start],
                end: offsets[lower_end],
            });
            // Advance past this match so overlapping ones are not reported
            // twice; the next byte is a char boundary because `needle` ends on
            // one.
            from = lower_end.max(lower_start + 1);
            if from >= haystack.len() {
                break;
            }
        }
    }

    out
}

fn lowercase(text: &str) -> String {
    text.chars().flat_map(|c| c.to_lowercase()).collect()
}

/// Lower-case `text`, and map each byte of the result back to a byte offset in
/// the original.
///
/// The two strings are not the same length — `İ` lower-cases to two chars, and
/// several characters change width — so a match found in the lower-cased copy
/// cannot be sliced out of the original without this map.
fn lowercase_with_offsets(text: &str) -> (String, Vec<usize>) {
    let mut lower = String::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(text.len() + 1);

    for (index, ch) in text.char_indices() {
        for lc in ch.to_lowercase() {
            lower.push(lc);
            // Every byte this character produced points at where the character
            // started, so a match landing anywhere inside it slices cleanly.
            offsets.resize(lower.len(), index);
        }
    }
    offsets.push(text.len());

    (lower, offsets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_query_is_found_in_the_run_that_holds_it() {
        let found = find_matches(&runs(&["hello world", "nothing here"]), "world");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].run, 0);
        assert_eq!(found[0].start, 6);
        assert_eq!(found[0].end, 11);
    }

    #[test]
    fn matching_ignores_case() {
        let found = find_matches(&runs(&["Hello World"]), "hello");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].start, found[0].end), (0, 5));
    }

    #[test]
    fn every_occurrence_in_a_run_is_reported() {
        let found = find_matches(&runs(&["ab ab ab"]), "ab");
        assert_eq!(found.len(), 3);
        assert_eq!(
            found.iter().map(|m| m.start).collect::<Vec<_>>(),
            vec![0, 3, 6]
        );
    }

    #[test]
    fn offsets_stay_valid_for_multibyte_text() {
        // The offsets have to index the original string, or slicing a match out
        // of Japanese text panics on a char boundary.
        let text = "日本語のテキスト";
        let found = find_matches(&runs(&[text.to_string().as_str()]), "テキスト");
        assert_eq!(found.len(), 1);
        assert_eq!(&text[found[0].start..found[0].end], "テキスト");
    }

    #[test]
    fn a_case_change_that_changes_length_still_slices_correctly() {
        // 'İ' lower-cases to two characters, so the lower-cased copy is longer
        // than the original and a raw offset from it would be wrong.
        let text = "İstanbul tour";
        let found = find_matches(&runs(&[text]), "tour");
        assert_eq!(found.len(), 1);
        assert_eq!(&text[found[0].start..found[0].end], "tour");
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(find_matches(&runs(&["anything"]), "").is_empty());
    }

    #[test]
    fn matches_come_back_in_document_order() {
        let found = find_matches(&runs(&["one x", "two x", "three x"]), "x");
        assert_eq!(
            found.iter().map(|m| m.run).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }
}
