use crate::*;

impl IdeallDesktop {
    pub(crate) fn refresh_sync_secret_status(&mut self) {
        match sync_secret_status(&self.secret_store) {
            Ok(secrets) => {
                self.sync_code_configured = secrets.sync_code_configured;
                self.sync_token_configured = secrets.sync_token_configured;
            }
            Err(error) => self.status = Some(error.into()),
        }
    }

    pub(crate) fn persist_sync_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PersistedSyncSettings, String> {
        let draft = SyncSettingsDraft {
            server_base_url: self.sync_server_input.read(cx).value().to_string(),
            sync_code: self.sync_code_input.read(cx).value().to_string(),
            bearer_token: self.sync_token_input.read(cx).value().to_string(),
        };
        let persisted = self
            .service
            .persist_sync_settings(&draft, &self.secret_store)?;
        self.sync_server_input.update(cx, |input, cx| {
            input.set_value(persisted.settings.server_base_url.clone(), window, cx);
        });
        if persisted.sync_code_saved {
            self.sync_code_configured = true;
            self.sync_code_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if persisted.bearer_token_saved {
            self.sync_token_configured = true;
            self.sync_token_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        Ok(persisted)
    }

    pub(crate) fn save_sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = match self.persist_sync_settings(window, cx) {
            Ok(_) => Some(SYNC_SETTINGS_SAVED_STATUS.into()),
            Err(error) => Some(error.into()),
        };
        cx.notify();
    }

    pub(crate) fn clear_sync_credentials(&mut self, cx: &mut Context<Self>) {
        match clear_sync_credentials(&self.secret_store) {
            Ok(()) => {
                self.sync_code_configured = false;
                self.sync_token_configured = false;
                self.status = Some(SYNC_CREDENTIALS_CLEARED_STATUS.into());
            }
            Err(error) => self.status = Some(error.into()),
        }
        cx.notify();
    }

    pub(crate) fn start_notes_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sync_in_progress {
            return;
        }
        let persisted = match self.persist_sync_settings(window, cx) {
            Ok(persisted) => persisted,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        let sync_code = match require_credential(&self.secret_store, CredentialKey::SyncCode) {
            Ok(value) => value,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        let token = match require_credential(&self.secret_store, CredentialKey::SyncBearerToken) {
            Ok(value) => value,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };

        let database_path = self.database_path.clone();
        let server_base_url = persisted.settings.server_base_url.clone();
        let task = cx.background_executor().spawn(async move {
            let mut workspace =
                LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
            let mut transport = HttpSyncTransport::new(&server_base_url, &token)
                .map_err(|error| error.to_string())?;
            workspace
                .sync_all_domains(&sync_code, &mut transport)
                .map_err(|error| error.to_string())
        });
        self.sync_in_progress = true;
        self.status = Some(SYNC_IN_PROGRESS_STATUS.into());
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.sync_in_progress = false;
                    match result {
                        Ok(summary) => {
                            this.refresh_items();
                            this.status = Some(summary.status_line().into());
                        }
                        Err(error) => this.status = Some(error.into()),
                    }
                    cx.notify();
                });
            })
        })
        .detach();
        cx.notify();
    }
}
