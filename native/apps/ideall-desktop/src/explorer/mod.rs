use super::*;
use std::collections::HashSet;

mod view;

pub(crate) use view::{DraggedNode, kind_color, kind_icon};

/// UI-only project explorer state, independent from the active editor.
pub(super) struct ExplorerState {
    pub(super) items: Vec<NodeSummary>,
    pub(super) selection_id: Option<String>,
    pub(super) expanded_folders: HashSet<String>,
    pub(super) panel_visible: bool,
    pub(super) panel_width: u16,
    pub(super) rename_input: Option<Entity<InputState>>,
    pub(super) renaming_id: Option<String>,
    pub(super) resize_state: Option<Entity<ResizableState>>,
}

impl ExplorerState {
    pub(super) fn new(
        items: Vec<NodeSummary>,
        layout: ExplorerLayoutState,
        rename_input: Entity<InputState>,
        resize_state: Entity<ResizableState>,
    ) -> Self {
        let expanded_folders = if layout.expanded_folders.is_empty() {
            items
                .iter()
                .filter(|item| item.kind == NodeKind::Folder)
                .map(|item| item.id.clone())
                .collect()
        } else {
            layout.expanded_folders.into_iter().collect()
        };
        Self {
            items,
            selection_id: None,
            expanded_folders,
            panel_visible: layout.visible,
            panel_width: layout.width.clamp(220, 520),
            rename_input: Some(rename_input),
            renaming_id: None,
            resize_state: Some(resize_state),
        }
    }

    pub(super) fn replace_items(&mut self, items: Vec<NodeSummary>) {
        self.items = items;
    }

    pub(super) fn select(&mut self, id: impl Into<String>) {
        self.selection_id = Some(id.into());
    }

    pub(super) fn clear_selection(&mut self) {
        self.selection_id = None;
    }

    pub(super) fn select_relative(&mut self, preserve_full_list: bool, offset: isize) -> bool {
        let visible = self.visible_items(preserve_full_list);
        if visible.is_empty() {
            return false;
        }
        let current = self
            .selection_id
            .as_deref()
            .and_then(|selected| visible.iter().position(|item| item.id == selected));
        let next = match current {
            Some(current) => {
                (current as isize + offset).clamp(0, visible.len() as isize - 1) as usize
            }
            None if offset < 0 => visible.len() - 1,
            None => 0,
        };
        self.selection_id = Some(visible[next].id.clone());
        true
    }

    pub(super) fn selected_folder_id(&self) -> Option<String> {
        self.selection_id
            .as_deref()
            .and_then(|selected_id| {
                self.items
                    .iter()
                    .find(|item| item.id == selected_id && item.kind == NodeKind::Folder)
            })
            .map(|item| item.id.clone())
    }

    pub(super) fn reveal_folder(&mut self, id: impl Into<String>) {
        self.expanded_folders.insert(id.into());
    }

    pub(super) fn toggle_folder(&mut self, id: String) {
        self.select(id.clone());
        if !self.expanded_folders.remove(&id) {
            self.expanded_folders.insert(id);
        }
    }

    #[cfg(test)]
    pub(super) fn expand_all(&mut self) {
        self.expanded_folders.extend(
            self.items
                .iter()
                .filter(|item| item.kind == NodeKind::Folder)
                .map(|item| item.id.clone()),
        );
    }

    #[cfg(test)]
    pub(super) fn collapse_all(&mut self) {
        self.expanded_folders.clear();
    }

    pub(super) fn toggle_panel(&mut self) {
        self.panel_visible = !self.panel_visible;
    }

    pub(super) fn update_panel_width(&mut self, width: f32) {
        self.panel_width = (width.round() as u16).clamp(220, 520);
    }

    pub(super) fn layout_state(&self) -> ExplorerLayoutState {
        let mut expanded_folders = self.expanded_folders.iter().cloned().collect::<Vec<_>>();
        expanded_folders.sort();
        ExplorerLayoutState {
            width: self.panel_width,
            expanded_folders,
            visible: self.panel_visible,
        }
    }

    pub(super) fn begin_rename(
        &mut self,
        id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) {
        self.select(id.clone());
        self.renaming_id = Some(id);
        if let Some(rename_input) = &self.rename_input {
            rename_input.update(cx, |input, cx| input.set_value(title, window, cx));
            rename_input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn cancel_rename(&mut self) {
        self.renaming_id = None;
    }

    pub(super) fn visible_items(&self, preserve_full_list: bool) -> Vec<NodeSummary> {
        if preserve_full_list {
            return self.items.clone();
        }

        let mut collapsed_depth = None;
        let mut visible = Vec::with_capacity(self.items.len());
        for item in &self.items {
            if let Some(depth) = collapsed_depth {
                if item.depth > depth {
                    continue;
                }
                collapsed_depth = None;
            }
            visible.push(item.clone());
            if item.kind == NodeKind::Folder && !self.expanded_folders.contains(&item.id) {
                collapsed_depth = Some(item.depth);
            }
        }
        visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explorer(items: Vec<NodeSummary>) -> ExplorerState {
        ExplorerState {
            items,
            selection_id: None,
            expanded_folders: HashSet::new(),
            panel_visible: true,
            panel_width: 272,
            rename_input: None,
            renaming_id: None,
            resize_state: None,
        }
    }

    fn summary(id: &str, parent_id: Option<&str>, kind: NodeKind, depth: usize) -> NodeSummary {
        NodeSummary {
            id: id.into(),
            parent_id: parent_id.map(str::to_owned),
            kind,
            title: id.into(),
            updated_at: 0,
            deleted: false,
            depth,
        }
    }

    #[test]
    fn collapsed_folders_hide_only_their_descendants() {
        let items = vec![
            summary("folder", None, NodeKind::Folder, 0),
            summary("nested", Some("folder"), NodeKind::Folder, 1),
            summary("file", Some("nested"), NodeKind::File, 2),
            summary("sibling", None, NodeKind::Note, 0),
        ];
        let mut explorer = explorer(items);
        explorer.collapse_all();

        assert_eq!(
            explorer
                .visible_items(false)
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["folder", "sibling"]
        );
    }

    #[test]
    fn expanded_folders_reveal_nested_project_tree() {
        let items = vec![
            summary("folder", None, NodeKind::Folder, 0),
            summary("nested", Some("folder"), NodeKind::Folder, 1),
            summary("file", Some("nested"), NodeKind::File, 2),
        ];
        let mut explorer = explorer(items.clone());
        explorer.expand_all();

        assert_eq!(explorer.visible_items(false), items);
    }

    #[test]
    fn selected_folder_controls_creation_parent() {
        let items = vec![
            summary("folder", None, NodeKind::Folder, 0),
            summary("file", Some("folder"), NodeKind::File, 1),
        ];
        let mut explorer = explorer(items);
        explorer.select("folder");
        assert_eq!(explorer.selected_folder_id().as_deref(), Some("folder"));

        explorer.select("file");
        assert_eq!(explorer.selected_folder_id(), None);
    }

    #[test]
    fn layout_state_is_stable_and_clamps_panel_width() {
        let mut explorer = explorer(Vec::new());
        explorer.expanded_folders = HashSet::from(["z".into(), "a".into()]);
        explorer.update_panel_width(900.0);
        explorer.panel_visible = false;

        assert_eq!(
            explorer.layout_state(),
            ExplorerLayoutState {
                width: 520,
                expanded_folders: vec!["a".into(), "z".into()],
                visible: false,
            }
        );
    }

    #[test]
    fn relative_selection_skips_hidden_descendants() {
        let items = vec![
            summary("folder", None, NodeKind::Folder, 0),
            summary("hidden", Some("folder"), NodeKind::File, 1),
            summary("sibling", None, NodeKind::Note, 0),
        ];
        let mut explorer = explorer(items);
        explorer.collapse_all();

        assert!(explorer.select_relative(false, 1));
        assert_eq!(explorer.selection_id.as_deref(), Some("folder"));
        assert!(explorer.select_relative(false, 1));
        assert_eq!(explorer.selection_id.as_deref(), Some("sibling"));
        assert!(explorer.select_relative(false, -1));
        assert_eq!(explorer.selection_id.as_deref(), Some("folder"));
    }
}
