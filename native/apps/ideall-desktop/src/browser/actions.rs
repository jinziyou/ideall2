use super::*;

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
}
