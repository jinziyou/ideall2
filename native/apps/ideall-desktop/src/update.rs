use crate::*;

impl IdeallDesktop {
    pub(crate) fn start_update_check(&mut self, cx: &mut Context<Self>) {
        if self.update_in_progress {
            return;
        }
        let task = cx.background_executor().spawn(async move {
            let updater = updater_from_environment().map_err(|error| error.to_string())?;
            updater
                .check(env!("CARGO_PKG_VERSION"))
                .map_err(|error| error.to_string())
        });
        self.update_in_progress = true;
        self.update_status = Some("正在获取并验证签名更新清单……".into());
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.update_in_progress = false;
                    match result {
                        Ok(UpdateStatus::Current { version }) => {
                            this.available_update = None;
                            this.update_status =
                                Some(format!("当前 {version} 已是最新版本").into());
                        }
                        Ok(UpdateStatus::Available(update)) => {
                            this.update_status =
                                Some(format!("已验证 {} 的签名更新清单", update.version).into());
                            this.available_update = Some(update);
                        }
                        Err(error) => this.update_status = Some(error.into()),
                    }
                    cx.notify();
                });
            })
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn start_update_download(&mut self, cx: &mut Context<Self>) {
        if self.update_in_progress {
            return;
        }
        let Some(update) = self.available_update.clone() else {
            self.update_status = Some("请先检查更新".into());
            cx.notify();
            return;
        };
        let destination_root = self
            .database_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("updates");
        let task = cx.background_executor().spawn(async move {
            let updater = updater_from_environment().map_err(|error| error.to_string())?;
            updater
                .download(&update, &destination_root)
                .map_err(|error| error.to_string())
        });
        self.update_in_progress = true;
        self.update_status = Some("正在下载并校验安装器大小与 SHA-256……".into());
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.update_in_progress = false;
                    match result {
                        Ok(path) => {
                            cx.open_with_system(&path);
                            this.update_status = Some(
                                "安装器已通过签名清单与 SHA-256 校验，并交给系统打开；安装前请保存当前编辑"
                                    .into(),
                            );
                        }
                        Err(error) => this.update_status = Some(error.into()),
                    }
                    cx.notify();
                });
            })
        })
        .detach();
        cx.notify();
    }
}
