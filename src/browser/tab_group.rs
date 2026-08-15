use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::tab::TabId;

/// A group of related tabs with a name and visual indicator.
#[derive(Debug)]
pub struct TabGroup {
    pub id: GroupId,
    pub name: String,
    pub color: GroupColor,
    pub tab_ids: SmallVec<[TabId; 4]>,
    /// Whether this group's tabs are visually collapsed (hidden) in the UI.
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupColor {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    Purple,
}

impl GroupColor {
    /// Returns all available group colors for cycling.
    pub fn variants() -> [GroupColor; 6] {
        [
            GroupColor::Blue,
            GroupColor::Green,
            GroupColor::Yellow,
            GroupColor::Orange,
            GroupColor::Red,
            GroupColor::Purple,
        ]
    }

    /// Returns the RGBA color for this group color as (r, g, b) in 0..=1 range.
    pub fn to_rgb(&self) -> (f32, f32, f32) {
        match self {
            GroupColor::Blue => (86.0 / 255.0, 170.0 / 255.0, 249.0 / 255.0),
            GroupColor::Green => (103.0 / 255.0, 211.0 / 255.0, 103.0 / 255.0),
            GroupColor::Yellow => (1.0, 220.0 / 255.0, 80.0 / 255.0),
            GroupColor::Orange => (1.0, 160.0 / 255.0, 60.0 / 255.0),
            GroupColor::Red => (249.0 / 255.0, 117.0 / 255.0, 117.0 / 255.0),
            GroupColor::Purple => (199.0 / 255.0, 128.0 / 255.0, 249.0 / 255.0),
        }
    }

    /// Returns a darker shade for the group header background.
    pub fn to_dark_rgb(&self) -> (f32, f32, f32) {
        let (r, g, b) = self.to_rgb();
        (r * 0.35, g * 0.35, b * 0.35)
    }
}

/// Manages tab groups with insertion-order iteration.
#[derive(Debug)]
pub struct GroupManager {
    next_id: u32,
    groups: FxHashMap<GroupId, TabGroup>,
    /// Tracks group insertion order for stable visual rendering.
    ordered_ids: Vec<GroupId>,
}

impl GroupManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            groups: FxHashMap::default(),
            ordered_ids: Vec::new(),
        }
    }

    /// Creates a new tab group.
    pub fn create_group(&mut self, name: String, color: GroupColor) -> GroupId {
        let id = GroupId(self.next_id);
        self.next_id += 1;
        self.groups.insert(
            id,
            TabGroup {
                id,
                name,
                color,
                tab_ids: SmallVec::new(),
                collapsed: false,
            },
        );
        self.ordered_ids.push(id);
        id
    }

    /// Adds a tab to a group. Returns `true` if the tab was added successfully.
    pub fn add_tab_to_group(&mut self, group_id: GroupId, tab_id: TabId) -> bool {
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.tab_ids.push(tab_id);
            true
        } else {
            false
        }
    }

    /// Removes a tab from a group. Returns `true` if the tab was present.
    pub fn remove_tab_from_group(&mut self, group_id: GroupId, tab_id: TabId) -> bool {
        if let Some(group) = self.groups.get_mut(&group_id) {
            let len_before = group.tab_ids.len();
            group.tab_ids.retain(|t| *t != tab_id);
            group.tab_ids.len() < len_before
        } else {
            false
        }
    }

    /// Removes a tab from whatever group it belongs to.
    pub fn remove_tab_from_any_group(&mut self, tab_id: TabId) {
        for group in self.groups.values_mut() {
            group.tab_ids.retain(|t| *t != tab_id);
        }
    }

    /// Looks up which group (if any) a tab belongs to.
    pub fn get_group_for_tab(&self, tab_id: TabId) -> Option<&TabGroup> {
        self.groups.values().find(|g| g.tab_ids.contains(&tab_id))
    }

    /// Toggles the collapsed state of a group.
    pub fn toggle_collapse(&mut self, id: GroupId) -> Option<bool> {
        if let Some(group) = self.groups.get_mut(&id) {
            group.collapsed = !group.collapsed;
            Some(group.collapsed)
        } else {
            None
        }
    }

    /// Deletes a group (tabs remain but are ungrouped).
    pub fn delete_group(&mut self, id: GroupId) {
        self.groups.remove(&id);
        self.ordered_ids.retain(|&gid| gid != id);
    }

    /// Returns all groups in insertion order.
    pub fn all_groups(&self) -> impl Iterator<Item = &TabGroup> {
        self.ordered_ids
            .iter()
            .filter_map(|&id| self.groups.get(&id))
    }

    /// Looks up a group by ID.
    pub fn get_group(&self, id: GroupId) -> Option<&TabGroup> {
        self.groups.get(&id)
    }

    /// Gets mutable access to a group by ID.
    pub fn get_group_mut(&mut self, id: GroupId) -> Option<&mut TabGroup> {
        self.groups.get_mut(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_group() {
        let mut manager = GroupManager::new();
        let id = manager.create_group("Work".to_string(), GroupColor::Blue);
        assert_eq!(manager.all_groups().count(), 1);
    }

    #[test]
    fn add_and_remove_tab() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Test".to_string(), GroupColor::Green);
        let tab = TabId::new(1);
        assert!(manager.add_tab_to_group(group, tab));
        assert!(manager.remove_tab_from_group(group, tab));
    }

    #[test]
    fn remove_tab_from_any_group() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Test".to_string(), GroupColor::Blue);
        let tab = TabId::new(5);
        manager.add_tab_to_group(group, tab);
        manager.remove_tab_from_any_group(tab);
        assert!(manager.get_group_for_tab(tab).is_none());
    }

    #[test]
    fn get_group_for_tab() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Work".to_string(), GroupColor::Purple);
        let tab = TabId::new(3);
        manager.add_tab_to_group(group, tab);
        assert!(manager.get_group_for_tab(tab).is_some());
        assert!(manager.get_group_for_tab(TabId::new(99)).is_none());
    }

    #[test]
    fn toggle_collapse() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Dev".to_string(), GroupColor::Orange);
        assert!(manager.toggle_collapse(group) == Some(true));
        assert!(manager.toggle_collapse(group) == Some(false));
    }

    #[test]
    fn all_groups_preserves_insertion_order() {
        let mut manager = GroupManager::new();
        let a = manager.create_group("First".to_string(), GroupColor::Blue);
        let _b = manager.create_group("Second".to_string(), GroupColor::Green);
        let ids: Vec<_> = manager.all_groups().map(|g| g.id).collect();
        assert_eq!(ids.first(), Some(&a));
    }

    #[test]
    fn delete_group_removes_from_order() {
        let mut manager = GroupManager::new();
        let one = manager.create_group("A".to_string(), GroupColor::Red);
        let _two = manager.create_group("B".to_string(), GroupColor::Yellow);
        manager.delete_group(one);
        assert_eq!(manager.all_groups().count(), 1);
    }

    #[test]
    fn group_color_to_rgb() {
        let (r, g, b) = GroupColor::Blue.to_rgb();
        assert!(r > 0.0 && r <= 1.0);
        assert!(b > g); // Blue should have most blue
    }
}
