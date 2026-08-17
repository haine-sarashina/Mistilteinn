//! Saved pages.
//!
//! Bookmarks outlive the process, so they are written to a file rather than
//! kept in memory like tabs are. The format is one `url \t title` line per
//! bookmark: a tab is the one character a URL cannot contain and a title is
//! stripped of, so no escaping is needed and the file stays readable and
//! repairable by hand.

use std::path::{Path, PathBuf};

/// One saved page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

/// The user's bookmarks, backed by a file.
#[derive(Debug, Default)]
pub struct BookmarkStore {
    items: Vec<Bookmark>,
    /// Where the list is saved. `None` when no writable location was found, in
    /// which case bookmarks still work for this session but are not kept.
    path: Option<PathBuf>,
}

/// Where bookmarks live: alongside the rest of this app's data.
fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(base)
            .join("Mistilteinn")
            .join("bookmarks.tsv"),
    )
}

impl BookmarkStore {
    /// Read the saved bookmarks, or start empty if there are none yet.
    pub fn load() -> Self {
        match default_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Read bookmarks from a specific file.
    pub fn load_from(path: &Path) -> Self {
        let items = std::fs::read_to_string(path)
            .map(|text| parse(&text))
            .unwrap_or_default();
        Self {
            items,
            path: Some(path.to_path_buf()),
        }
    }

    /// The bookmarks, newest last.
    pub fn items(&self) -> &[Bookmark] {
        &self.items
    }

    /// Whether this URL is already saved.
    pub fn contains(&self, url: &str) -> bool {
        self.items.iter().any(|b| b.url == url)
    }

    /// Save the page if it is not saved, remove it if it is.
    ///
    /// Returns whether the page is bookmarked afterwards — the caller uses that
    /// to decide what the star looks like.
    pub fn toggle(&mut self, url: &str, title: &str) -> bool {
        if url.trim().is_empty() {
            return false;
        }
        let added = if self.contains(url) {
            self.items.retain(|b| b.url != url);
            false
        } else {
            self.items.push(Bookmark {
                url: url.to_string(),
                title: clean_title(title, url),
            });
            true
        };
        self.save();
        added
    }

    /// Write the list out, creating the directory if it is not there yet.
    ///
    /// A failure is logged and otherwise ignored: losing bookmarks is bad, but
    /// refusing to browse because a file could not be written is worse.
    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("could not create the bookmark directory {dir:?}: {e}");
                return;
            }
        }
        if let Err(e) = std::fs::write(path, serialize(&self.items)) {
            log::warn!("could not save bookmarks to {path:?}: {e}");
        }
    }

    /// The bookmark list as a page, for the internal `mistilteinn://bookmarks`
    /// URL. Built as HTML so the engine renders it like any other document.
    pub fn to_html(&self) -> String {
        let mut body = String::new();
        if self.items.is_empty() {
            body.push_str("<p class='empty'>ブックマークはまだありません。Ctrl+D で現在のページを保存できます。</p>");
        }
        for bookmark in self.items.iter().rev() {
            body.push_str(&format!(
                "<li><a href=\"{}\">{}</a><div class='url'>{}</div></li>",
                escape_html(&bookmark.url),
                escape_html(&bookmark.title),
                escape_html(&bookmark.url),
            ));
        }

        format!(
            "<!DOCTYPE html><html><head><meta charset=\"UTF-8\"><title>ブックマーク</title>\
             <style>\
             body {{ background: #202124; color: #e8eaed; margin: 0; padding: 48px; \
             font-family: -apple-system, \"Segoe UI\", Roboto, sans-serif; }}\
             h1 {{ font-size: 22px; font-weight: 500; margin: 0 0 24px 0; }}\
             ul {{ list-style: none; margin: 0; padding: 0; }}\
             li {{ padding: 10px 0; border-bottom: 1px solid #3c4043; }}\
             a {{ color: #8ab4f8; text-decoration: none; font-size: 15px; }}\
             .url {{ color: #9aa0a6; font-size: 12px; margin-top: 2px; }}\
             .empty {{ color: #9aa0a6; font-size: 14px; }}\
             </style></head><body><h1>ブックマーク ({})</h1><ul>{}</ul></body></html>",
            self.items.len(),
            body
        )
    }
}

/// A title worth showing in the list.
///
/// Pages without a `<title>` land here as "New Tab", which says nothing about
/// what was saved; the URL at least identifies it.
fn clean_title(title: &str, url: &str) -> String {
    let title = title.replace(['\t', '\n', '\r'], " ");
    let title = title.trim();
    if title.is_empty() || title == "New Tab" {
        url.to_string()
    } else {
        title.to_string()
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn serialize(items: &[Bookmark]) -> String {
    let mut out = String::new();
    for bookmark in items {
        out.push_str(&bookmark.url);
        out.push('\t');
        out.push_str(&bookmark.title);
        out.push('\n');
    }
    out
}

/// Read the file back, skipping anything malformed rather than failing.
fn parse(text: &str) -> Vec<Bookmark> {
    text.lines()
        .filter_map(|line| {
            let (url, title) = line.split_once('\t')?;
            let url = url.trim();
            if url.is_empty() {
                return None;
            }
            Some(Bookmark {
                url: url.to_string(),
                title: title.trim().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> BookmarkStore {
        BookmarkStore::default()
    }

    #[test]
    fn toggling_adds_then_removes_a_page() {
        let mut store = store();
        assert!(store.toggle("https://example.com/", "Example"));
        assert!(store.contains("https://example.com/"));
        assert_eq!(store.items().len(), 1);

        assert!(!store.toggle("https://example.com/", "Example"));
        assert!(!store.contains("https://example.com/"));
        assert!(store.items().is_empty());
    }

    #[test]
    fn a_page_with_no_title_is_listed_by_its_url() {
        let mut store = store();
        store.toggle("https://example.com/x", "New Tab");
        assert_eq!(store.items()[0].title, "https://example.com/x");
    }

    #[test]
    fn an_empty_url_is_not_saved() {
        // The star is clickable on a blank new tab, which has no URL to save.
        let mut store = store();
        assert!(!store.toggle("   ", "Untitled"));
        assert!(store.items().is_empty());
    }

    #[test]
    fn a_saved_list_survives_a_round_trip() {
        let items = vec![
            Bookmark {
                url: "https://a.example/".to_string(),
                title: "A".to_string(),
            },
            Bookmark {
                url: "https://b.example/path?q=1".to_string(),
                title: "B の記事".to_string(),
            },
        ];
        assert_eq!(parse(&serialize(&items)), items);
    }

    #[test]
    fn a_tab_in_a_title_does_not_split_the_record() {
        // The separator has to be removed from the title, or reading the file
        // back finds a field boundary that was never intended.
        let mut store = store();
        store.toggle("https://example.com/", "before\tafter");
        assert_eq!(parse(&serialize(store.items())), store.items());
        assert_eq!(store.items()[0].title, "before after");
    }

    #[test]
    fn malformed_lines_are_skipped_rather_than_failing_the_load() {
        let parsed = parse("no-tab-here\nhttps://ok.example/\tOK\n\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].url, "https://ok.example/");
    }

    #[test]
    fn the_bookmark_page_lists_saved_titles_and_urls() {
        let mut store = store();
        store.toggle("https://example.com/", "Example & Co");
        let html = store.to_html();
        assert!(html.contains("href=\"https://example.com/\""));
        assert!(
            html.contains("Example &amp; Co"),
            "titles are escaped into the page: {html}"
        );
    }

    #[test]
    fn bookmarks_are_written_and_read_back_from_disk() {
        let path =
            std::env::temp_dir().join(format!("mistilteinn-bookmarks-{}.tsv", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut store = BookmarkStore::load_from(&path);
        store.toggle("https://example.com/", "Example");

        let reloaded = BookmarkStore::load_from(&path);
        assert_eq!(reloaded.items().len(), 1);
        assert_eq!(reloaded.items()[0].url, "https://example.com/");

        let _ = std::fs::remove_file(&path);
    }
}
