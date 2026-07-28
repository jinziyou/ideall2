use ideall_protocol::FileRef;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    #[default]
    Files,
    Audio,
    Development,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabDescriptor {
    pub file: FileRef,
    pub engine_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation_path: Option<String>,
}

pub fn tab_key(descriptor: &TabDescriptor) -> String {
    let file = descriptor
        .file
        .to_key()
        .expect("a persisted tab must contain a valid FileRef");
    let engine = FileRef::new("engine", &descriptor.engine_id)
        .to_key()
        .expect("an engine id must be non-empty");
    format!("file:{file}?{engine}")
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    pub kind: WorkspaceKind,
    pub tabs: Vec<TabDescriptor>,
    pub active_id: Option<String>,
    #[serde(default)]
    pub explorer: ExplorerLayoutState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerLayoutState {
    #[serde(default = "default_explorer_width")]
    pub width: u16,
    #[serde(default)]
    pub expanded_folders: Vec<String>,
    #[serde(default = "default_true")]
    pub visible: bool,
}

impl Default for ExplorerLayoutState {
    fn default() -> Self {
        Self {
            width: default_explorer_width(),
            expanded_folders: Vec::new(),
            visible: true,
        }
    }
}

const fn default_explorer_width() -> u16 {
    272
}

const fn default_true() -> bool {
    true
}

impl WorkspaceState {
    pub fn open(&mut self, descriptor: TabDescriptor) -> &str {
        let id = tab_key(&descriptor);
        if let Some(existing) = self
            .tabs
            .iter_mut()
            .find(|existing| tab_key(existing) == id)
        {
            existing.title = descriptor.title;
            existing.root_id = descriptor.root_id;
            existing.navigation_path = descriptor.navigation_path;
        } else {
            self.tabs.push(descriptor);
        }
        self.active_id = Some(id);
        self.active_id
            .as_deref()
            .expect("active id was just assigned")
    }

    pub fn activate(&mut self, id: &str) -> bool {
        if self.tabs.iter().any(|tab| tab_key(tab) == id) {
            self.active_id = Some(id.to_owned());
            true
        } else {
            false
        }
    }

    pub fn close(&mut self, id: &str) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab_key(tab) == id) else {
            return false;
        };
        self.tabs.remove(index);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map(tab_key);
        }
        true
    }

    pub fn activate_next(&mut self) -> bool {
        self.activate_relative(1)
    }

    pub fn activate_previous(&mut self) -> bool {
        self.activate_relative(-1)
    }

    fn activate_relative(&mut self, offset: isize) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        let current = self
            .active_id
            .as_deref()
            .and_then(|active| self.tabs.iter().position(|tab| tab_key(tab) == active))
            .unwrap_or(0);
        let len = self.tabs.len() as isize;
        let next = (current as isize + offset).rem_euclid(len) as usize;
        self.active_id = self.tabs.get(next).map(tab_key);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(file_id: &str, engine_id: &str, title: &str) -> TabDescriptor {
        TabDescriptor {
            file: FileRef::new("local.nodes", file_id),
            engine_id: engine_id.into(),
            title: title.into(),
            root_id: None,
            navigation_path: None,
        }
    }

    #[test]
    fn same_file_and_engine_reuses_tab_but_another_engine_does_not() {
        let mut workspace = WorkspaceState::default();
        let first = workspace.open(tab("n1", "preview", "Old")).to_owned();
        let second = workspace.open(tab("n1", "preview", "New")).to_owned();
        let third = workspace.open(tab("n1", "code", "Code")).to_owned();

        assert_eq!(first, second);
        assert_ne!(second, third);
        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(workspace.tabs[0].title, "New");
    }

    #[test]
    fn closing_active_tab_selects_a_neighbor() {
        let mut workspace = WorkspaceState::default();
        let first = workspace.open(tab("a", "preview", "A")).to_owned();
        let second = workspace.open(tab("b", "preview", "B")).to_owned();
        assert!(workspace.close(&second));
        assert_eq!(workspace.active_id.as_deref(), Some(first.as_str()));
    }

    #[test]
    fn relative_tab_activation_wraps_in_both_directions() {
        let mut state = WorkspaceState::default();
        let first = state
            .open(TabDescriptor {
                file: FileRef::new("local", "first"),
                engine_id: "ideall.note".into(),
                title: "First".into(),
                root_id: None,
                navigation_path: None,
            })
            .to_owned();
        let second = state
            .open(TabDescriptor {
                file: FileRef::new("local", "second"),
                engine_id: "ideall.note".into(),
                title: "Second".into(),
                root_id: None,
                navigation_path: None,
            })
            .to_owned();

        assert!(state.activate_next());
        assert_eq!(state.active_id.as_deref(), Some(first.as_str()));
        assert!(state.activate_previous());
        assert_eq!(state.active_id.as_deref(), Some(second.as_str()));
    }

    #[test]
    fn legacy_workspace_json_uses_lightweight_explorer_defaults() {
        let state: WorkspaceState =
            serde_json::from_str(r#"{"kind":"files","tabs":[],"activeId":null}"#).unwrap();

        assert_eq!(state.explorer, ExplorerLayoutState::default());
        assert_eq!(state.explorer.width, 272);
        assert!(state.explorer.visible);
    }
}
