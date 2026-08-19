//! The system clipboard.
//!
//! One place for every cut, copy and paste in the browser, so the address bar,
//! the find bar, a page's text field and a script's `navigator.clipboard` all
//! reach the same board. A test — and a machine with no window server — gets an
//! in-process board instead, which is what makes the paths that use it testable
//! at all.

use std::cell::RefCell;

thread_local! {
    /// A board of our own, used when the system has none to offer.
    static FALLBACK: RefCell<String> = const { RefCell::new(String::new()) };
    /// Set by tests, so nothing touches the machine's real clipboard.
    static USE_FALLBACK: RefCell<bool> = const { RefCell::new(false) };
}

/// Send every read and write to the in-process board rather than the system's.
pub fn use_in_process_board(enabled: bool) {
    USE_FALLBACK.with(|flag| *flag.borrow_mut() = enabled);
}

fn in_process_only() -> bool {
    USE_FALLBACK.with(|flag| *flag.borrow())
}

/// What is on the clipboard, if it holds text.
pub fn read_text() -> Option<String> {
    if !in_process_only()
        && let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(text) = clipboard.get_text()
    {
        return Some(text);
    }
    FALLBACK.with(|board| {
        let text = board.borrow().clone();
        (!text.is_empty()).then_some(text)
    })
}

/// Put text on the clipboard. Returns whether it got there.
///
/// The in-process board is written either way, so a copy followed by a paste
/// behaves the same when the system clipboard is unavailable.
pub fn write_text(text: &str) -> bool {
    FALLBACK.with(|board| *board.borrow_mut() = text.to_string());
    if in_process_only() {
        return true;
    }
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.to_string())) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("could not reach the system clipboard: {error}");
            false
        }
    }
}

/// Text pasted into a single-line field.
///
/// A URL copied out of a document often arrives wrapped over several lines;
/// pasting the line breaks would make a field that shows one line show part of
/// one. Everything is joined and the ends trimmed, which is what browsers do.
pub fn as_single_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test here uses the in-process board: touching the machine's real
    /// clipboard would destroy whatever the person running the tests had on it.
    fn isolated() {
        use_in_process_board(true);
        FALLBACK.with(|board| board.borrow_mut().clear());
    }

    #[test]
    fn what_is_written_comes_back() {
        isolated();
        assert!(write_text("https://example.com"));
        assert_eq!(read_text().as_deref(), Some("https://example.com"));
    }

    #[test]
    fn an_empty_board_reads_as_nothing() {
        isolated();
        assert_eq!(read_text(), None);
    }

    #[test]
    fn writing_again_replaces_what_was_there() {
        isolated();
        write_text("first");
        write_text("second");
        assert_eq!(read_text().as_deref(), Some("second"));
    }

    #[test]
    fn a_wrapped_url_pastes_as_one_line() {
        assert_eq!(
            as_single_line("https://example.com/\n  a/very/long/path"),
            "https://example.com/a/very/long/path"
        );
    }

    #[test]
    fn single_line_text_is_only_trimmed() {
        assert_eq!(as_single_line("  hello  "), "hello");
        assert_eq!(as_single_line("hello world"), "hello world");
    }
}
