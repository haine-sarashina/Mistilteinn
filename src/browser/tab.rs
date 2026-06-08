use crate::page::Page;
use rustc_hash::FxHashMap;

use super::tab_group::GroupId;

/// Represents a single browser tab.
///
/// Each tab holds its own browsing context including DOM, layout tree,
/// and scroll position.
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub url: String,
    pub page: Option<Page>,
    pub scroll_offset: (f32, f32),
    pub is_loading: bool,
    pub history: Vec<String>,
    pub history_index: usize,
    /// Which group this tab belongs to, if any.
    pub group_id: Option<GroupId>,
}

impl std::fmt::Debug for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tab")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("url", &self.url)
            .field("page", &self.page.is_some())
            .field("scroll_offset", &self.scroll_offset)
            .field("history_len", &self.history.len())
            .field("history_index", &self.history_index)
            .field("group_id", &self.group_id.map(|g| g.0))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u32);

impl TabId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

impl Tab {
    /// Add a URL to the history, truncating any forward entries if we're not at the end.
    pub fn push_history(&mut self, url: &str) {
        // If there are forward entries (index is not at the last position), truncate them
        if !self.history.is_empty() && self.history_index < self.history.len() - 1 {
            self.history.truncate(self.history_index);
        }
        self.history.push(url.to_string());
        self.history_index = self.history.len() - 1;
    }

    /// Check if there are entries to navigate back to.
    pub fn can_go_back(&self) -> bool {
        self.history_index > 0 && !self.history.is_empty()
    }

    /// Check if there are entries to navigate forward to.
    pub fn can_go_forward(&self) -> bool {
        self.history_index < self.history.len() - 1
    }

    /// Navigate back in history, returning the URL at the new position.
    pub fn go_back(&mut self) -> Option<String> {
        if self.history_index == 0 || self.history.is_empty() {
            return None;
        }
        self.history_index -= 1;
        Some(self.history[self.history_index].clone())
    }

    /// Navigate forward in history, returning the URL at the new position.
    pub fn go_forward(&mut self) -> Option<String> {
        if self.history_index >= self.history.len() - 1 {
            return None;
        }
        self.history_index += 1;
        Some(self.history[self.history_index].clone())
    }
}

/// Manages all open tabs and their organization into groups.
#[derive(Debug)]
pub struct TabManager {
    next_id: u32,
    tabs: FxHashMap<TabId, Tab>,
    active_tab: Option<TabId>,
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            tabs: FxHashMap::default(),
            active_tab: None,
        }
    }

    /// Creates a new tab and returns its ID.
    pub fn create_tab(&mut self) -> TabId {
        let id = TabId::new(self.next_id);
        self.next_id += 1;
        self.tabs.insert(
            id,
            Tab {
                id,
                title: "New Tab".to_string(),
                url: String::new(),
                page: None,
                scroll_offset: (0.0, 0.0),
                is_loading: false,
                history: Vec::new(),
                history_index: 0,
                group_id: None,
            },
        );
        self.active_tab = Some(id);
        id
    }

    /// Assigns a tab to a group, returning the old group if any.
    pub fn assign_to_group(&mut self, tab_id: TabId, group_id: GroupId) -> Option<GroupId> {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.group_id.replace(group_id)
        } else {
            None
        }
    }

    /// Unassigns a tab from its group.
    pub fn unassign_group(&mut self, tab_id: TabId) -> Option<GroupId> {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.group_id.take()
        } else {
            None
        }
    }

    /// Closes a tab by ID.
    pub fn close_tab(&mut self, id: TabId) {
        self.tabs.remove(&id);
        if self.active_tab == Some(id) {
            self.active_tab = self.tabs.keys().next().copied();
        }
    }

    /// Switches the active tab.
    pub fn activate_tab(&mut self, id: TabId) {
        if self.tabs.contains_key(&id) {
            self.active_tab = Some(id);
        }
    }

    /// Returns a reference to the active tab, if any.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab.and_then(|id| self.tabs.get(&id))
    }

    /// Returns all tabs.
    pub fn all_tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.values()
    }

    /// Gets a specific tab by ID.
    pub fn get_tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.get(&id)
    }

    /// Get a mutable reference to the active tab, if any.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active_tab.and_then(|id| self.tabs.get_mut(&id))
    }

    /// Set the page on the active tab.
    pub fn set_active_tab_page(&mut self, page: Page) {
        if let Some(tab) = self.active_tab_mut() {
            tab.page = Some(page);
        }
    }

    /// Get a reference to the active tab's page, if any.
    pub fn get_active_tab_page(&self) -> Option<&Page> {
        self.active_tab().and_then(|t| t.page.as_ref())
    }

    /// Get a mutable reference to the active tab's scroll offset, if any.
    pub fn get_active_tab_scroll_mut(&mut self) -> Option<&mut (f32, f32)> {
        self.active_tab_mut().map(|t| &mut t.scroll_offset)
    }

    /// Returns the ID of the currently active tab, if any.
    pub fn active_tab_id(&self) -> Option<TabId> {
        self.active_tab
    }

    /// Returns whether the active tab is currently loading a page.
    pub fn is_active_tab_loading(&self) -> bool {
        self.active_tab().map(|t| t.is_loading).unwrap_or(false)
    }

    /// Sets the loading state of the active tab.
    pub fn set_active_tab_loading(&mut self, loading: bool) {
        if let Some(tab) = self.active_tab_mut() {
            tab.is_loading = loading;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_has_page_field() {
        let mut manager = TabManager::new();
        let _id = manager.create_tab();
        let tab = manager.active_tab().expect("tab exists");
        assert!(tab.page.is_none(), "New tab should have None for page");
    }

    #[test]
    fn test_tab_history_push_and_navigate() {
        let mut manager = TabManager::new();
        let _id = manager.create_tab();
        let tab = manager.active_tab_mut().expect("tab exists");
        tab.push_history("https://a.com");
        tab.push_history("https://b.com");
        tab.push_history("https://c.com");

        assert!(tab.can_go_back());
        assert!(!tab.can_go_forward());

        let url = tab.go_back();
        assert_eq!(url, Some("https://b.com".to_string()));
        assert!(tab.can_go_back());
        assert!(tab.can_go_forward());

        let url = tab.go_forward();
        assert_eq!(url, Some("https://c.com".to_string()));
    }

    #[test]
    fn test_tab_history_truncates_forward() {
        let mut manager = TabManager::new();
        let _id = manager.create_tab();
        let tab = manager.active_tab_mut().expect("tab exists");
        tab.push_history("https://a.com");
        tab.push_history("https://b.com");
        tab.push_history("https://c.com");

        // Go back one (now at "b", index=1)
        tab.go_back();
        assert!(tab.can_go_forward());

        // Push a new URL - this should truncate forward history
        tab.push_history("https://d.com");
        assert!(!tab.can_go_forward());
        assert_eq!(tab.history.len(), 2); // [a, d]
    }

    #[test]
    fn test_tab_manager_active_tab_page() {
        let mut manager = TabManager::new();
        let _id = manager.create_tab();

        assert!(manager.get_active_tab_page().is_none());

        let page = Page::new(
            "<html><body><div>Hello</div></body></html>",
            "div { display: block; }",
            800.0,
            600.0,
        );
        manager.set_active_tab_page(page);

        assert!(manager.get_active_tab_page().is_some());
    }

    #[test]
    fn create_and_close_tab() {
        let mut manager = TabManager::new();
        let id = manager.create_tab();
        assert_eq!(manager.all_tabs().count(), 1);
        manager.close_tab(id);
        assert_eq!(manager.all_tabs().count(), 0);
    }

    #[test]
    fn switch_active_tab() {
        let mut manager = TabManager::new();
        let tab1 = manager.create_tab();
        let _tab2 = manager.create_tab();
        manager.activate_tab(tab1);
        assert_eq!(manager.active_tab().map(|t| t.id), Some(tab1));
    }

    #[test]
    fn new_tab_has_no_group() {
        let manager = TabManager::new();
        assert!(manager.all_tabs().next().is_none());
    }
}
