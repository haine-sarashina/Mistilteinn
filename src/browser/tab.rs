use rustc_hash::FxHashMap;

/// Represents a single browser tab.
///
/// Each tab holds its own browsing context including DOM, layout tree,
/// and scroll position.
#[derive(Debug)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub url: String,
    // TODO: DOM tree
    // TODO: Layout tree
    // TODO: Render state
    // TODO: Scroll position
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u32);

impl TabId {
    pub fn new(id: u32) -> Self {
        Self(id)
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
            },
        );
        self.active_tab = Some(id);
        id
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let tab2 = manager.create_tab();
        manager.activate_tab(tab1);
        assert_eq!(manager.active_tab().map(|t| t.id), Some(tab1));
    }
}
