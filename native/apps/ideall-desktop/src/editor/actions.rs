use super::*;

impl EditorWorkspace {
    pub(crate) fn active_tab_id(&self) -> Option<String> {
        self.workspace.active_id.clone()
    }

    pub(crate) fn activate_tab(&mut self, id: &str) -> Option<String> {
        self.workspace.activate(id).then(|| {
            self.workspace
                .tabs
                .iter()
                .find(|tab| tab_key(tab) == id)
                .map(|tab| tab.file.file_id.clone())
        })?
    }

    pub(crate) fn activate_next_tab(&mut self) -> Option<String> {
        self.workspace.activate_next().then(|| {
            self.workspace.active_id.as_deref().and_then(|active_id| {
                self.workspace
                    .tabs
                    .iter()
                    .find(|tab| tab_key(tab) == active_id)
                    .map(|tab| tab.file.file_id.clone())
            })
        })?
    }

    pub(crate) fn activate_previous_tab(&mut self) -> Option<String> {
        self.workspace.activate_previous().then(|| {
            self.workspace.active_id.as_deref().and_then(|active_id| {
                self.workspace
                    .tabs
                    .iter()
                    .find(|tab| tab_key(tab) == active_id)
                    .map(|tab| tab.file.file_id.clone())
            })
        })?
    }
}
