use super::*;
use gpui_component::input::Position;

impl IdeallDesktop {
    pub(crate) fn navigate_browser_to(
        &mut self,
        value: &str,
        record_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = match normalize_browser_url(value) {
            Ok(url) => url,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        if record_history {
            self.browser.record_navigation(url.clone());
        } else {
            self.browser.current_url = Some(url.clone());
        }
        // gpui-component 单行 set_value 会把光标移到末尾，地址被横向滚动截断；
        // 回到文本开头后恢复先前焦点。
        let previous_focus = window.focused(cx);
        self.browser.address_input.update(cx, |input, cx| {
            input.set_value(url.clone(), window, cx);
            input.set_cursor_position(Position::new(0, 0), window, cx);
        });
        if let Some(handle) = previous_focus {
            window.focus(&handle);
        }
        self.browser.webview = None;
        match create_embedded_browser(&url, window, cx) {
            Ok(webview) => {
                self.browser.webview = Some(webview);
                self.status =
                    Some("Browser 已在隔离 WebView 中打开；未注入本地文件或密钥能力".into());
            }
            Err(error) => {
                cx.open_url(&url);
                self.status = Some(format!("{error}；已交给系统浏览器").into());
            }
        }
        cx.notify();
    }

    pub(crate) fn navigate_browser_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = self.browser.go_back() else {
            return;
        };
        self.navigate_browser_to(&url, false, window, cx);
    }

    pub(crate) fn navigate_browser_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = self.browser.go_forward() else {
            return;
        };
        self.navigate_browser_to(&url, false, window, cx);
    }

    pub(crate) fn reload_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = self.browser.current_url().map(str::to_owned) else {
            return;
        };
        self.navigate_browser_to(&url, false, window, cx);
    }

    pub(crate) fn open_current_browser_external(&mut self, cx: &mut Context<Self>) {
        let Some(url) = self.browser.current_url().map(str::to_owned) else {
            self.status = Some("当前没有可打开的网页地址".into());
            cx.notify();
            return;
        };
        cx.open_url(&url);
        self.status = Some("已交给系统浏览器打开".into());
        cx.notify();
    }

    pub(crate) fn capture_browser_bookmark(&mut self, cx: &mut Context<Self>) {
        let value = self.browser.address_input.read(cx).value().to_string();
        let url = match normalize_browser_url(&value) {
            Ok(url) => url,
            Err(error) => {
                self.status = Some(format!("无法存入收集箱：{error}").into());
                cx.notify();
                return;
            }
        };
        match self.service.capture_bookmark(&url) {
            Ok(node) => {
                self.refresh_items();
                self.status = Some("已存入收集箱".into());
                self.fetch_bookmark_metadata(node.base().id.clone(), url, cx);
            }
            Err(error) => self.status = Some(format!("无法存入收集箱：{error}").into()),
        }
        cx.notify();
    }

    /// 捕获成功后后台补全书签元数据；抓取或合并失败完全静默，
    /// 不改动「已存入收集箱」的状态提示。
    pub(crate) fn fetch_bookmark_metadata(
        &mut self,
        id: String,
        url: String,
        cx: &mut Context<Self>,
    ) {
        let fetch = cx.background_executor().spawn(async move {
            ideall_webmeta::fetch_page_meta(&url).map(|meta| BookmarkMetadata {
                canonical_url: meta.canonical_url,
                title: meta.title,
                description: meta.description,
                favicon: meta.favicon,
            })
        });
        cx.spawn(async move |view, cx| {
            let result = fetch.await;
            let _ = cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    let Ok(meta) = result else {
                        return;
                    };
                    if meta.canonical_url.is_none()
                        && meta.title.is_none()
                        && meta.description.is_none()
                        && meta.favicon.is_none()
                    {
                        return;
                    }
                    if this.service.apply_bookmark_metadata(&id, &meta).is_ok() {
                        this.refresh_items();
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }
}
