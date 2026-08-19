//! Web Storage: `localStorage` and `sessionStorage`.
//!
//! Both are the same thing with different lifetimes. A storage area belongs to
//! an origin, so two pages on one site share what they save and a page on
//! another site cannot see it — that separation is the whole point of the API,
//! and it is enforced here by never handing a page an area it did not ask for
//! by origin.
//!
//! `localStorage` outlives the browser and so is written to disk;
//! `sessionStorage` belongs to one tab and dies with it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustc_hash::FxHashMap;

/// The per-origin limit browsers apply, in bytes of key plus value.
///
/// Storage is a shared resource with no prompt in front of it, so a page that
/// writes without bound would fill the user's disk. Five megabytes is the
/// figure every browser settled on.
pub const QUOTA_BYTES: usize = 5 * 1024 * 1024;

/// One origin's key/value store.
///
/// Insertion order is kept because `key(n)` is part of the API: a script can
/// walk the store by index, and the order it gets has to be stable between
/// calls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageArea {
    entries: Vec<(String, String)>,
    /// Bumped by every change, so the persistence layer can tell whether there
    /// is anything to write without comparing the whole store.
    revision: u64,
}

/// What went wrong with a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    /// The write would take this origin past [`QUOTA_BYTES`].
    QuotaExceeded,
}

impl StorageArea {
    pub fn new() -> Self {
        Self::default()
    }

    /// The value stored under `key`, if any.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Store a value, replacing any previous one under the same key.
    ///
    /// A replacement keeps the key's place in the order, as the spec says: only
    /// a genuinely new key goes on the end.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        let existing = self.entries.iter().position(|(name, _)| name == key);
        let freed = existing
            .map(|index| entry_size(&self.entries[index]))
            .unwrap_or(0);
        let added = key.len() + value.len();
        if self.bytes() - freed + added > QUOTA_BYTES {
            return Err(StorageError::QuotaExceeded);
        }

        match existing {
            Some(index) => self.entries[index].1 = value.to_string(),
            None => self.entries.push((key.to_string(), value.to_string())),
        }
        self.revision += 1;
        Ok(())
    }

    /// Forget one key. Removing a key that is not there is not an error.
    pub fn remove(&mut self, key: &str) {
        let before = self.entries.len();
        self.entries.retain(|(name, _)| name != key);
        if self.entries.len() != before {
            self.revision += 1;
        }
    }

    /// Forget everything this origin saved.
    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.entries.clear();
            self.revision += 1;
        }
    }

    /// The `index`th key, in insertion order.
    pub fn key_at(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|(key, _)| key.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many bytes of key and value this origin is using.
    pub fn bytes(&self) -> usize {
        self.entries.iter().map(entry_size).sum()
    }

    /// How many times this area has changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The entries, in insertion order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }
}

fn entry_size(entry: &(String, String)) -> usize {
    entry.0.len() + entry.1.len()
}

/// A storage area shared between a page and whatever owns its lifetime.
pub type SharedStorageArea = Rc<RefCell<StorageArea>>;

/// The storage areas one page can see.
#[derive(Clone, Default)]
pub struct PageStorage {
    pub local: SharedStorageArea,
    pub session: SharedStorageArea,
}

/// The origin a URL's storage belongs to.
///
/// A URL with no origin — an internal page, a `data:` URI, a document with no
/// URL at all — gets one bucket named for what it is rather than being lumped
/// in with the last real site visited.
pub fn storage_origin(url: &str) -> String {
    match crate::network::security::Origin::parse(url) {
        Some(origin) => origin.serialize(),
        None => "opaque".to_string(),
    }
}

/// Every origin's `localStorage`, and the file they live in.
#[derive(Debug, Default)]
pub struct LocalStorageStore {
    areas: FxHashMap<String, SharedStorageArea>,
    path: Option<PathBuf>,
    /// The total revision count at the last successful write.
    saved_revision: u64,
}

/// Where `localStorage` is kept, beside the bookmarks.
fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(base)
            .join("Mistilteinn")
            .join("localstorage.tsv"),
    )
}

impl LocalStorageStore {
    /// Read what previous sessions saved, or start empty.
    pub fn load() -> Self {
        match default_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    pub fn load_from(path: &Path) -> Self {
        let mut store = Self {
            areas: FxHashMap::default(),
            path: Some(path.to_path_buf()),
            saved_revision: 0,
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return store;
        };
        for line in text.lines() {
            let mut fields = line.split('\t');
            let (Some(origin), Some(key), Some(value)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let area = store.area_for(&unescape(origin));
            // A file that has been edited by hand could be over quota; the
            // entries that fit are kept rather than dropping the origin.
            let _ = area.borrow_mut().set(&unescape(key), &unescape(value));
        }
        store.saved_revision = store.total_revision();
        store
    }

    /// The area for an origin, creating an empty one the first time.
    pub fn area_for(&mut self, origin: &str) -> SharedStorageArea {
        self.areas.entry(origin.to_string()).or_default().clone()
    }

    /// Write to disk, but only if something has changed since the last write.
    ///
    /// Called at the points where a script has just had a chance to run rather
    /// than on every frame: the file is small, but writing it sixty times a
    /// second would not be.
    pub fn save_if_changed(&mut self) {
        let revision = self.total_revision();
        if revision == self.saved_revision {
            return;
        }
        if self.save().is_ok() {
            self.saved_revision = revision;
        }
    }

    fn total_revision(&self) -> u64 {
        self.areas
            .values()
            .map(|area| area.borrow().revision())
            .sum()
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Origins in a stable order, so the file does not churn between runs
        // for reasons that have nothing to do with what a page saved.
        let mut origins: Vec<&String> = self.areas.keys().collect();
        origins.sort();

        let mut out = String::new();
        for origin in origins {
            let area = self.areas[origin].borrow();
            for (key, value) in area.entries() {
                out.push_str(&escape(origin));
                out.push('\t');
                out.push_str(&escape(key));
                out.push('\t');
                out.push_str(&escape(value));
                out.push('\n');
            }
        }
        std::fs::write(path, out)
    }
}

/// Escape the characters the file format uses as separators.
///
/// Anything can be stored under any key, tabs and newlines included, so the
/// separators have to be recoverable.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// The `sessionStorage` of one tab: one area per origin it has visited.
#[derive(Debug, Default)]
pub struct SessionStorageStore {
    areas: FxHashMap<String, SharedStorageArea>,
}

impl SessionStorageStore {
    pub fn area_for(&mut self, origin: &str) -> SharedStorageArea {
        self.areas.entry(origin.to_string()).or_default().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_comes_back_under_the_key_it_was_stored_with() {
        let mut area = StorageArea::new();
        area.set("theme", "dark").unwrap();
        assert_eq!(area.get("theme"), Some("dark"));
        assert_eq!(area.get("missing"), None);
    }

    #[test]
    fn keys_are_walked_in_the_order_they_were_added() {
        let mut area = StorageArea::new();
        area.set("a", "1").unwrap();
        area.set("b", "2").unwrap();
        area.set("c", "3").unwrap();
        assert_eq!(area.key_at(0), Some("a"));
        assert_eq!(area.key_at(2), Some("c"));
        assert_eq!(area.key_at(3), None);
        assert_eq!(area.len(), 3);
    }

    #[test]
    fn replacing_a_value_keeps_the_keys_place_in_the_order() {
        let mut area = StorageArea::new();
        area.set("a", "1").unwrap();
        area.set("b", "2").unwrap();
        area.set("a", "rewritten").unwrap();
        assert_eq!(area.key_at(0), Some("a"));
        assert_eq!(area.get("a"), Some("rewritten"));
        assert_eq!(area.len(), 2);
    }

    #[test]
    fn removing_a_key_that_is_not_there_is_not_an_error() {
        let mut area = StorageArea::new();
        area.set("a", "1").unwrap();
        area.remove("b");
        area.remove("a");
        assert_eq!(area.len(), 0);
    }

    #[test]
    fn clearing_empties_the_whole_area() {
        let mut area = StorageArea::new();
        area.set("a", "1").unwrap();
        area.set("b", "2").unwrap();
        area.clear();
        assert!(area.is_empty());
        assert_eq!(area.get("a"), None);
    }

    #[test]
    fn a_write_past_the_quota_is_refused_and_changes_nothing() {
        let mut area = StorageArea::new();
        let big = "x".repeat(QUOTA_BYTES - 10);
        area.set("k", &big).unwrap();
        assert_eq!(
            area.set("second", "more than ten bytes over"),
            Err(StorageError::QuotaExceeded)
        );
        assert_eq!(area.len(), 1, "the refused write left no trace");
    }

    #[test]
    fn overwriting_a_large_value_reclaims_the_space_it_held() {
        let mut area = StorageArea::new();
        area.set("k", &"x".repeat(QUOTA_BYTES - 100)).unwrap();
        // The same key again is not an extra five megabytes.
        assert!(area.set("k", &"y".repeat(QUOTA_BYTES - 100)).is_ok());
    }

    #[test]
    fn only_a_real_change_bumps_the_revision() {
        let mut area = StorageArea::new();
        let start = area.revision();
        area.remove("never-existed");
        area.clear();
        assert_eq!(area.revision(), start);
        area.set("a", "1").unwrap();
        assert!(area.revision() > start);
    }

    #[test]
    fn storage_is_bucketed_by_origin_not_by_url() {
        assert_eq!(
            storage_origin("https://example.com/one"),
            storage_origin("https://example.com/two/three?q=1")
        );
        assert_ne!(
            storage_origin("https://example.com/"),
            storage_origin("http://example.com/"),
            "a different scheme is a different origin"
        );
        assert_ne!(
            storage_origin("https://example.com/"),
            storage_origin("https://other.example.com/")
        );
    }

    #[test]
    fn a_page_with_no_origin_gets_its_own_bucket() {
        assert_eq!(storage_origin(""), "opaque");
        assert_eq!(storage_origin("mistilteinn://bookmarks"), "opaque");
    }

    #[test]
    fn separators_survive_a_round_trip_through_the_file_format() {
        for text in ["plain", "with\ttab", "with\nnewline", "back\\slash", "\\t"] {
            assert_eq!(unescape(&escape(text)), text, "for {text:?}");
        }
    }

    /// A store written to a temporary file and read back.
    fn round_trip(write: impl FnOnce(&mut LocalStorageStore)) -> LocalStorageStore {
        let path = std::env::temp_dir().join(format!(
            "mistilteinn-storage-test-{}.tsv",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = LocalStorageStore::load_from(&path);
        write(&mut store);
        store.save_if_changed();
        let reloaded = LocalStorageStore::load_from(&path);
        let _ = std::fs::remove_file(&path);
        reloaded
    }

    #[test]
    fn local_storage_outlives_the_session_that_wrote_it() {
        let reloaded = round_trip(|store| {
            let area = store.area_for("https://example.com");
            area.borrow_mut().set("token", "abc123").unwrap();
        });
        let mut reloaded = reloaded;
        let area = reloaded.area_for("https://example.com");
        assert_eq!(area.borrow().get("token"), Some("abc123"));
    }

    #[test]
    fn one_origin_cannot_read_anothers_storage() {
        let mut reloaded = round_trip(|store| {
            store
                .area_for("https://a.example")
                .borrow_mut()
                .set("secret", "mine")
                .unwrap();
            store
                .area_for("https://b.example")
                .borrow_mut()
                .set("other", "theirs")
                .unwrap();
        });
        assert_eq!(
            reloaded.area_for("https://a.example").borrow().get("other"),
            None
        );
        assert_eq!(
            reloaded
                .area_for("https://a.example")
                .borrow()
                .get("secret"),
            Some("mine")
        );
    }

    #[test]
    fn awkward_keys_and_values_survive_being_saved() {
        let mut reloaded = round_trip(|store| {
            store
                .area_for("https://example.com")
                .borrow_mut()
                .set("a\tkey", "a\nvalue\\here")
                .unwrap();
        });
        assert_eq!(
            reloaded
                .area_for("https://example.com")
                .borrow()
                .get("a\tkey"),
            Some("a\nvalue\\here")
        );
    }

    #[test]
    fn two_pages_on_one_origin_share_an_area() {
        let mut store = LocalStorageStore::default();
        let first = store.area_for("https://example.com");
        let second = store.area_for("https://example.com");
        first.borrow_mut().set("shared", "yes").unwrap();
        assert_eq!(second.borrow().get("shared"), Some("yes"));
    }

    #[test]
    fn a_tabs_session_storage_is_its_own() {
        let mut one = SessionStorageStore::default();
        let mut other = SessionStorageStore::default();
        one.area_for("https://example.com")
            .borrow_mut()
            .set("k", "v")
            .unwrap();
        assert_eq!(
            other.area_for("https://example.com").borrow().get("k"),
            None
        );
    }
}
