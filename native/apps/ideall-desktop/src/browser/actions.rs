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
        self.browser
            .address_input
            .update(cx, |input, cx| input.set_value(url.clone(), window, cx));
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
            Ok(_) => {
                self.refresh_items();
                self.status = Some("已存入收集箱".into());
            }
            Err(error) => self.status = Some(format!("无法存入收集箱：{error}").into()),
        }
        cx.notify();
    }
}
