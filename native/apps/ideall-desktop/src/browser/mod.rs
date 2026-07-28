use super::*;
use url::Url;

mod actions;

const MAX_BROWSER_HISTORY: usize = 100;
const MAX_URL_CHARS: usize = 8 * 1024;

/// Owns browser navigation state without adding a standalone workbench page.
///
/// Bookmark and portal flows borrow this component's WebView. Navigation,
/// history and URL validation therefore remain independent from editor state.
pub(super) struct BrowserWorkspace {
    current_url: Option<String>,
    webview: Option<Entity<WebView>>,
    history: BrowserHistory,
}

#[derive(Default)]
struct BrowserHistory {
    entries: Vec<String>,
    index: Option<usize>,
}

impl BrowserWorkspace {
    pub(super) fn new() -> Self {
        Self {
            current_url: None,
            webview: None,
            history: BrowserHistory::default(),
        }
    }

    fn record_navigation(&mut self, url: String) {
        if self.current_url.as_deref() == Some(url.as_str()) {
            return;
        }
        self.history.record(url.clone());
        self.current_url = Some(url);
    }

    pub(super) fn clear(&mut self) {
        self.current_url = None;
        self.webview = None;
    }

    pub(super) fn webview(&self) -> Option<Entity<WebView>> {
        self.webview.clone()
    }
}

impl BrowserHistory {
    fn record(&mut self, url: String) {
        if let Some(index) = self.index {
            self.entries.truncate(index + 1);
        }
        self.entries.push(url);
        if self.entries.len() > MAX_BROWSER_HISTORY {
            self.entries.remove(0);
        }
        self.index = Some(self.entries.len() - 1);
    }
}

fn normalize_browser_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MAX_URL_CHARS
        || value.chars().any(char::is_control)
    {
        return Err("地址必须包含 1–8192 个非控制字符".into());
    }
    let candidate = if value.contains("://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let url = Url::parse(&candidate).map_err(|_| "无法解析网页地址".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Browser 只允许 HTTP(S) 地址".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Browser 不接受 URL 中的用户名或密码".into());
    }
    Ok(url.into())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn create_embedded_browser(
    url: &str,
    window: &mut Window,
    cx: &mut Context<IdeallDesktop>,
) -> Result<Entity<WebView>, String> {
    let webview = wry::WebViewBuilder::new()
        .with_url(url)
        .build_as_child(window)
        .map_err(|error| error.to_string())?;
    Ok(cx.new(|cx| WebView::new(webview, window, cx)))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn create_embedded_browser(
    _url: &str,
    _window: &mut Window,
    _cx: &mut Context<IdeallDesktop>,
) -> Result<Entity<WebView>, String> {
    Err("当前 Linux GPUI 后端不支持可靠的子窗口 WebView；可使用“系统浏览器打开”".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_urls_default_to_https_and_reject_unsafe_schemes_or_credentials() {
        assert_eq!(
            normalize_browser_url("example.com/path").unwrap(),
            "https://example.com/path"
        );
        assert!(normalize_browser_url("javascript:alert(1)").is_err());
        assert!(normalize_browser_url("https://user:secret@example.com").is_err());
    }

    #[test]
    fn browser_history_truncates_forward_entries_after_new_navigation() {
        let mut history = BrowserHistory::default();
        history.record("https://a.example/".into());
        history.record("https://b.example/".into());
        history.index = Some(0);
        history.record("https://c.example/".into());

        assert_eq!(
            history.entries,
            vec!["https://a.example/", "https://c.example/"]
        );
        assert_eq!(history.index, Some(1));
    }
}
