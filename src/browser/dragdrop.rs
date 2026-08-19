//! Dragging and dropping: what is being carried, and who is carrying it.
//!
//! HTML's drag and drop is a state machine spread over seven events, and the
//! one thing all of them share is the `DataTransfer` — the parcel being moved.
//! It lives here rather than in the JavaScript engine, because a drop can also
//! come from outside the browser, when the reader drags a file onto the window
//! and there is no page script involved in picking it up.

use std::cell::RefCell;
use std::path::PathBuf;

/// The parcel a drag is carrying.
///
/// Formats are keyed as the API keys them: `"text/plain"`, `"text/uri-list"`,
/// and the shorthands `"text"` and `"url"` that older pages use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DragData {
    items: Vec<(String, String)>,
    /// Paths dropped from outside the browser.
    pub files: Vec<PathBuf>,
    /// What the source says may happen: `copy`, `move`, `link`, `all`, `none`.
    pub effect_allowed: String,
    /// What the target says will happen.
    pub drop_effect: String,
}

/// Normalise the shorthand format names to the media types they stand for.
fn canonical_format(format: &str) -> String {
    match format.trim().to_ascii_lowercase().as_str() {
        "text" => "text/plain".to_string(),
        "url" => "text/uri-list".to_string(),
        other => other.to_string(),
    }
}

impl DragData {
    pub fn get(&self, format: &str) -> String {
        let format = canonical_format(format);
        self.items
            .iter()
            .find(|(name, _)| *name == format)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    pub fn set(&mut self, format: &str, value: &str) {
        let format = canonical_format(format);
        match self.items.iter_mut().find(|(name, _)| *name == format) {
            Some(entry) => entry.1 = value.to_string(),
            None => self.items.push((format, value.to_string())),
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The formats this parcel holds, in the order they were set.
    pub fn types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.items.iter().map(|(name, _)| name.clone()).collect();
        if !self.files.is_empty() {
            types.push("Files".to_string());
        }
        types
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.files.is_empty()
    }
}

thread_local! {
    /// The parcel of the drag in progress. There is only ever one.
    static CURRENT: RefCell<DragData> = const { RefCell::new(DragData {
        items: Vec::new(),
        files: Vec::new(),
        effect_allowed: String::new(),
        drop_effect: String::new(),
    }) };
    static IN_PROGRESS: RefCell<bool> = const { RefCell::new(false) };
}

/// Start carrying something. Whatever the last drag held is forgotten.
pub fn begin(files: Vec<PathBuf>) {
    CURRENT.with(|current| {
        *current.borrow_mut() = DragData {
            items: Vec::new(),
            files,
            effect_allowed: "all".to_string(),
            drop_effect: "none".to_string(),
        }
    });
    IN_PROGRESS.with(|flag| *flag.borrow_mut() = true);
}

/// Put down whatever was being carried.
pub fn end() {
    CURRENT.with(|current| *current.borrow_mut() = DragData::default());
    IN_PROGRESS.with(|flag| *flag.borrow_mut() = false);
}

pub fn in_progress() -> bool {
    IN_PROGRESS.with(|flag| *flag.borrow())
}

/// Look at the parcel.
pub fn with_data<R>(read: impl FnOnce(&DragData) -> R) -> R {
    CURRENT.with(|current| read(&current.borrow()))
}

/// Change the parcel.
pub fn with_data_mut<R>(change: impl FnOnce(&mut DragData) -> R) -> R {
    CURRENT.with(|current| change(&mut current.borrow_mut()))
}

/// How far the pointer must travel with a button held before a press becomes
/// a drag.
///
/// Without a threshold every click on a draggable element would start one, and
/// a page that styles its drag source would flicker on every click.
pub const DRAG_THRESHOLD: f32 = 4.0;

/// Whether the pointer has moved far enough from where it was pressed.
pub fn past_threshold(pressed_at: (f32, f32), now_at: (f32, f32)) -> bool {
    let (dx, dy) = (now_at.0 - pressed_at.0, now_at.1 - pressed_at.1);
    (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD
}

/// The drag the browser is running, as the window sees it.
#[derive(Debug, Clone, Default)]
pub struct DragState {
    /// The element pressed, if it or an ancestor is draggable.
    pub candidate: Option<u32>,
    /// Where the press happened, in window coordinates.
    pub pressed_at: (f32, f32),
    /// The element the drag started from, once it has started.
    pub source: Option<u32>,
    /// The element the pointer is over.
    pub over: Option<u32>,
    /// Whether the element under the pointer has agreed to accept a drop, by
    /// cancelling the last `dragover`. This is the rule HTML actually uses,
    /// and it is why a page that forgets `preventDefault` never gets a drop.
    pub will_accept: bool,
    pub active: bool,
}

impl DragState {
    /// Forget everything, as the end of a drag does.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> DragData {
        end();
        begin(Vec::new());
        with_data(|data| data.clone())
    }

    #[test]
    fn what_is_set_comes_back() {
        fresh();
        with_data_mut(|data| data.set("text/plain", "hello"));
        assert_eq!(with_data(|data| data.get("text/plain")), "hello");
    }

    #[test]
    fn a_format_that_was_never_set_reads_as_empty() {
        fresh();
        assert_eq!(with_data(|data| data.get("text/html")), "");
    }

    #[test]
    fn the_shorthand_names_mean_the_media_types_they_stand_for() {
        let mut data = DragData::default();
        data.set("text", "written as text");
        assert_eq!(data.get("text/plain"), "written as text");
        data.set("url", "https://example.com");
        assert_eq!(data.get("text/uri-list"), "https://example.com");
    }

    #[test]
    fn setting_a_format_twice_replaces_it() {
        let mut data = DragData::default();
        data.set("text/plain", "first");
        data.set("text/plain", "second");
        assert_eq!(data.get("text/plain"), "second");
        assert_eq!(data.types(), vec!["text/plain"]);
    }

    #[test]
    fn files_show_up_in_the_type_list() {
        let mut data = DragData {
            files: vec![PathBuf::from("/tmp/a.txt")],
            ..Default::default()
        };
        data.set("text/plain", "x");
        assert_eq!(data.types(), vec!["text/plain", "Files"]);
    }

    #[test]
    fn ending_a_drag_puts_down_what_it_was_carrying() {
        fresh();
        with_data_mut(|data| data.set("text/plain", "carried"));
        assert!(in_progress());
        end();
        assert!(!in_progress());
        assert!(with_data(|data| data.is_empty()));
    }

    #[test]
    fn a_new_drag_does_not_inherit_the_last_ones_parcel() {
        fresh();
        with_data_mut(|data| data.set("text/plain", "old"));
        begin(Vec::new());
        assert_eq!(with_data(|data| data.get("text/plain")), "");
    }

    #[test]
    fn a_file_drop_starts_out_carrying_the_files() {
        begin(vec![
            PathBuf::from("/tmp/a.txt"),
            PathBuf::from("/tmp/b.txt"),
        ]);
        assert_eq!(with_data(|data| data.files.len()), 2);
        assert!(with_data(|data| data
            .types()
            .contains(&"Files".to_string())));
    }

    #[test]
    fn a_press_is_not_a_drag_until_the_pointer_has_moved() {
        assert!(!past_threshold((100.0, 100.0), (100.0, 100.0)));
        assert!(!past_threshold((100.0, 100.0), (102.0, 100.0)));
        assert!(past_threshold((100.0, 100.0), (110.0, 100.0)));
        assert!(past_threshold((100.0, 100.0), (100.0, 90.0)));
    }

    #[test]
    fn resetting_clears_the_whole_state() {
        let mut state = DragState {
            candidate: Some(3),
            source: Some(3),
            over: Some(7),
            will_accept: true,
            active: true,
            pressed_at: (1.0, 2.0),
        };
        state.reset();
        assert!(state.candidate.is_none());
        assert!(!state.active);
        assert!(!state.will_accept);
    }
}
