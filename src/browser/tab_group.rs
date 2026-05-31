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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupColor {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    Purple,
}

/// Manages tab groups.
#[derive(Debug)]
pub struct GroupManager {
    next_id: u32,
    groups: FxHashMap<GroupId, TabGroup>,
}

impl GroupManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            groups: FxHashMap::default(),
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
            },
        );
        id
    }

    /// Adds a tab to a group.
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

    /// Deletes a group (tabs remain but are ungrouped).
    pub fn delete_group(&mut self, id: GroupId) {
        self.groups.remove(&id);
    }

    /// Returns all groups.
    pub fn all_groups(&self) -> impl Iterator<Item = &TabGroup> {
        self.groups.values()
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
        assert_eq!(manager.groups[&id].name, "Work");
    }

    #[test]
    fn add_and_remove_tab() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Test".to_string(), GroupColor::Green);
        let tab = TabId::new(1);
        assert!(manager.add_tab_to_group(group, tab));
        assert!(manager.groups[&group].tab_ids.contains(&tab));
        assert!(manager.remove_tab_from_group(group, tab));
        assert!(!manager.groups[&group].tab_ids.contains(&tab));
    }
}
