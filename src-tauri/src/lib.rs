// 内嵌浏览器 (连接模式, 路线 A): 主窗口内嵌原生子 webview 显示外站, 工具条由可信本地前端渲染。
// Linux/WSL: Tauri add_child 默认 gtk::Box 堆叠 (子 webview 落窗口底部) → browser_linux.rs 用 gtk::Fixed 精确定位。
// 非 Linux: Window::add_child + 前端 getBoundingClientRect 同步 bounds。切走标签 hide; WSLg 内嵌浏览器或需 GDK_BACKEND=x11。
#[cfg(all(desktop, not(target_os = "linux")))]
use tauri::webview::{PageLoadEvent, WebviewBuilder};
#[cfg(desktop)]
use tauri::AppHandle;
#[cfg(desktop)]
use tauri::Manager;
#[cfg(desktop)]
use tauri::WebviewUrl;
#[cfg(all(desktop, not(target_os = "linux")))]
use tauri::{Emitter, LogicalPosition, LogicalSize, Rect};

// ACP (Agent Client Protocol) 外部智能体传输 —— 子进程 stdin/stdout 哑管道 (NDJSON 行框定), 仅桌面。
#[cfg(desktop)]
mod acp_transport;
#[cfg(all(desktop, target_os = "linux"))]
mod browser_cdp;
#[cfg(all(desktop, target_os = "linux"))]
mod browser_linux;
#[cfg(desktop)]
mod browser_scripts;
#[cfg(all(desktop, target_os = "windows"))]
mod browser_win;
mod capture_share;
#[cfg(desktop)]
mod extension_registry;
#[cfg(desktop)]
mod guarded_fs;
#[cfg(desktop)]
mod installed_apps;
#[cfg(desktop)]
mod oauth_callback;
#[cfg(desktop)]
mod runtime_extensions;
#[cfg(desktop)]
mod secure_store;
#[cfg(desktop)]
mod window_placement;

#[cfg(all(desktop, not(target_os = "linux")))]
const BROWSER_LABEL: &str = "browser_view";

/// Windows: 上次 present 的 bounds, 供页面加载后再次同步。
#[cfg(all(desktop, target_os = "windows"))]
#[derive(Default)]
struct BrowserWinState {
    last_bounds: std::sync::Mutex<Option<Bounds>>,
    subclassed: std::sync::Mutex<Vec<isize>>,
}

/// Tauri invoke 在后台线程; GTK / WebView2 / with_webview 必须在主线程同步完成。
#[cfg(all(desktop, any(target_os = "windows", target_os = "linux")))]
fn run_on_main_thread_sync<R, F>(app: &AppHandle, f: F) -> Result<R, String>
where
    R: Send + 'static,
    F: FnOnce(&AppHandle) -> Result<R, String> + Send + 'static,
{
    #[cfg(target_os = "linux")]
    {
        use gtk::glib::MainContext;
        if MainContext::default().is_owner() {
            return f(app);
        }
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let app = app.clone();
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&app2));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|_| "主线程执行失败".to_string())?
}

/// 取主窗口。勿用 get_webview_window("main"): add_child 挂上 "browser" 子 webview 后
/// 窗口不再满足 is_webview_window() (全部 webview label == 窗口 label) → 永远返回 None,
/// 导致浏览器打开后所有窗控/布局命令报「主窗口不存在」。
#[cfg(desktop)]
fn main_window(app: &AppHandle) -> Result<tauri::Window, String> {
    app.get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())
}

// 主窗口内容区矩形 (CSS 像素, 相对窗口左上)。直接当 Logical 传, 勿乘 devicePixelRatio (Tauri 自动按 scale 换算)。
#[cfg(desktop)]
#[derive(serde::Deserialize, Clone, Copy, Debug, Default)]
pub(crate) struct Bounds {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// 子 webview 是原生 HWND (Windows) / NSView, 盖住主 webview 顶栏; 校验并裁剪 bounds。
#[cfg(all(desktop, not(target_os = "linux")))]
fn sanitize_browser_bounds(app: &AppHandle, mut b: Bounds) -> Result<Bounds, String> {
    // TopBar(44) + TabBar(36) + browser 卡片上边距/工具条(≈64) ≈ 144; 留 4px 容差。
    const MIN_Y: f64 = 140.0;
    const MIN_X: f64 = 48.0;
    const MIN_SIZE: f64 = 8.0;
    if b.w < MIN_SIZE || b.h < MIN_SIZE {
        return Err(format!("浏览器区域过小 ({}×{})", b.w, b.h));
    }
    if b.y < MIN_Y {
        return Err(format!(
            "浏览器区域过高 (y={}), 会遮挡顶栏/窗控",
            b.y.round()
        ));
    }
    if b.x < MIN_X {
        return Err(format!("浏览器区域过左 (x={}), 会遮挡侧栏", b.x.round()));
    }
    let main = main_window(app)?;
    let scale = main.scale_factor().unwrap_or(1.0);
    if let Ok(inner) = main.inner_size() {
        let win_w = inner.width as f64 / scale;
        let win_h = inner.height as f64 / scale;
        if b.w > win_w * 0.92 || b.h > win_h * 0.88 {
            return Err(format!(
                "浏览器区域过大 ({}×{}), 会遮挡壳层 UI",
                b.w.round(),
                b.h.round()
            ));
        }
        if b.x + b.w > win_w + 1.0 {
            b.w = (win_w - b.x).max(MIN_SIZE);
        }
        if b.y + b.h > win_h + 1.0 {
            b.h = (win_h - b.y).max(MIN_SIZE);
        }
    }
    Ok(b)
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn browser_webview_builder(
    label: &str,
    parsed: tauri::Url,
    app_nav: AppHandle,
    app_load: AppHandle,
) -> WebviewBuilder<tauri::Wry> {
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(parsed))
        .on_navigation(move |target| {
            let _ = app_nav.emit("browser://url", target.to_string());
            true
        })
        .on_page_load(move |_webview, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                let _ = app_load.emit("browser://url", payload.url().to_string());
                #[cfg(target_os = "windows")]
                {
                    let app = app_load.clone();
                    let app2 = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        browser_win_reclip(&app2);
                    });
                }
            }
        });
    // 内网自签 HTTPS (如 inventory); Linux 走 webkit TLS policy, Windows 需 WebView2 参数。
    #[cfg(target_os = "windows")]
    {
        builder = builder.additional_browser_args("--ignore-certificate-errors");
    }
    builder
}

/// 移出视口并隐藏 (WebView2 hide 后有时仍拦截鼠标, 须归零尺寸)。
#[cfg(all(desktop, not(target_os = "linux")))]
fn stash_browser_webview(wv: &tauri::Webview) {
    let _ = wv.hide();
    let _ = wv.set_size(LogicalSize::new(0.0, 0.0));
    let _ = wv.set_position(LogicalPosition::new(-10000.0, -10000.0));
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn bounds_to_rect(b: Bounds) -> Rect {
    Rect {
        position: LogicalPosition::new(b.x, b.y).into(),
        size: LogicalSize::new(b.w, b.h).into(),
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn apply_browser_bounds(wv: &tauri::Webview, app: &AppHandle, b: Bounds) -> Result<Bounds, String> {
    let b = sanitize_browser_bounds(app, b)?;
    wv.set_bounds(bounds_to_rect(b))
        .map_err(|e| e.to_string())?;
    Ok(b)
}

#[cfg(all(desktop, target_os = "windows"))]
fn win_webview_hwnd(wv: &tauri::Webview) -> Option<isize> {
    use std::sync::{Arc, Mutex};
    let out = Arc::new(Mutex::new(None::<isize>));
    let slot = out.clone();
    let _ = wv.with_webview(move |platform| {
        let controller = platform.controller();
        let mut hwnd = windows::Win32::Foundation::HWND::default();
        unsafe {
            let _ = controller.ParentWindow(std::ptr::addr_of_mut!(hwnd) as _);
        }
        if !hwnd.0.is_null() {
            *slot.lock().unwrap() = Some(hwnd.0 as isize);
        }
    });
    let hwnd = out.lock().unwrap().take();
    hwnd
}

#[cfg(all(desktop, target_os = "windows"))]
fn browser_win_clear_subclass(app: &AppHandle) {
    let hwnds = app
        .state::<BrowserWinState>()
        .subclassed
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    browser_win::remove_hit_pass_tree(&hwnds);
    if let Ok(mut list) = app.state::<BrowserWinState>().subclassed.lock() {
        list.clear();
    }
}

#[cfg(all(desktop, target_os = "windows"))]
fn browser_win_sync(wv: &tauri::Webview, app: &AppHandle, b: Bounds) {
    let Ok(main_win) = main_window(app) else {
        return;
    };
    let scale = main_win.scale_factor().unwrap_or(1.0);
    // 命中区 = 内容矩形 (屏幕坐标); 矩形外 HTTRANSPARENT → 顶栏/窗控/右侧 AI 可点。
    let hit_y = b.y.max(browser_win::TOP_CHROME_LOGICAL);
    let hit_h = (b.y + b.h - hit_y).max(0.0);
    let local = browser_win::PhysicalBounds::from_logical(b.x, hit_y, b.w, hit_h, scale);
    let (origin_x, origin_y) = if let Some(main) = app.get_webview("main") {
        win_webview_hwnd(&main)
            .and_then(browser_win::client_origin_screen)
            .unwrap_or((0, 0))
    } else {
        (0, 0)
    };
    let hit_rect = browser_win::PhysicalBounds {
        x: origin_x + local.x,
        y: origin_y + local.y,
        w: local.w,
        h: local.h,
    };
    // 子 HWND 自身定位仍用内容 bounds (与 set_bounds 一致)。
    let pb = browser_win::PhysicalBounds::from_logical(b.x, b.y, b.w, b.h, scale);

    browser_win_clear_subclass(app);

    let installed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<isize>::new()));
    let slot = installed.clone();
    let b_log = b;

    let _ = wv.with_webview(move |platform| {
        let controller = platform.controller();
        let mut hwnd = windows::Win32::Foundation::HWND::default();
        unsafe {
            let _ = controller.ParentWindow(std::ptr::addr_of_mut!(hwnd) as _);
        }
        if hwnd.0.is_null() {
            eprintln!("[ideall] browser sync: ParentWindow null");
            return;
        }
        let hwnd_raw = hwnd.0 as isize;

        if let Err(e) = browser_win::force_hwnd_bounds(hwnd_raw, pb) {
            eprintln!("[ideall] browser sync SetWindowPos: {e}");
        }
        unsafe {
            use windows::Win32::Foundation::RECT;
            let _ = controller.SetBounds(RECT {
                left: 0,
                top: 0,
                right: pb.w,
                bottom: pb.h,
            });
            let _ = controller.NotifyParentWindowPositionChanged();
        }

        if let Some(rect) = browser_win::hwnd_screen_rect(hwnd_raw) {
            eprintln!(
                "[ideall] browser sync: logical=({:.0},{:.0} {:.0}x{:.0}) physical=({},{} {}x{}) hwnd_rect={:?} hit_rect=({},{} {}x{})",
                b_log.x, b_log.y, b_log.w, b_log.h, pb.x, pb.y, pb.w, pb.h, rect,
                hit_rect.x, hit_rect.y, hit_rect.w, hit_rect.h
            );
        }

        let list = browser_win::install_hit_pass_tree(hwnd_raw, hit_rect);
        eprintln!(
            "[ideall] browser sync: hit_pass on {} hwnd(s)",
            list.len()
        );
        *slot.lock().unwrap() = list;
    });

    if let Ok(mut list) = app.state::<BrowserWinState>().subclassed.lock() {
        *list = installed.lock().unwrap().clone();
    }
}

#[cfg(all(desktop, target_os = "windows"))]
fn browser_win_reclip(app: &AppHandle) {
    let b = match app.state::<BrowserWinState>().last_bounds.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    let Some(b) = b else { return };
    if let Some(wv) = app.get_webview(BROWSER_LABEL) {
        browser_win_sync(&wv, app, b);
    }
}

#[cfg(all(desktop, target_os = "windows"))]
fn win_raise_main_webview(app: &AppHandle) {
    let Some(main) = app.get_webview("main") else {
        return;
    };
    if let Some(hwnd) = win_webview_hwnd(&main) {
        browser_win::raise_hwnd(hwnd);
    }
}

/// 内嵌浏览器后端信息 (CDP / WebKit / WebView)。
#[cfg(desktop)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserBackendInfo {
    /// `cdp` | `webkit` (Linux) | `webview` (Win/macOS)
    mode: String,
    cdp_available: bool,
    running: bool,
    chrome_path: Option<String>,
}

/// 内嵌浏览器当前页快照 (agent browser.getContent 用)。
#[cfg(desktop)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserPageContent {
    url: String,
    title: String,
    text: String,
    selection: String,
}

#[cfg(all(desktop, not(target_os = "linux")))]
const BROWSER_CONTENT_JS: &str = browser_scripts::CONTENT_JS;

#[cfg(all(desktop, not(target_os = "linux")))]
pub(crate) fn parse_browser_page_json(
    url: String,
    json_str: &str,
) -> Result<BrowserPageContent, String> {
    let v: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("解析页面内容失败: {e}"))?;
    if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
        if !err.is_empty() {
            return Err(format!("页面脚本错误: {err}"));
        }
    }
    Ok(BrowserPageContent {
        url,
        title: v["title"].as_str().unwrap_or("").to_string(),
        text: v["text"].as_str().unwrap_or("").to_string(),
        selection: v["selection"].as_str().unwrap_or("").to_string(),
    })
}

#[cfg(desktop)]
pub(crate) fn parse_http_url(url: &str) -> Result<tauri::Url, String> {
    let u: tauri::Url = url.parse().map_err(|e| format!("非法网址: {e}"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err("仅支持 http/https".into());
    }
    Ok(u)
}

// ── 内嵌浏览器 DOM 操控 (agent browser.click / fill / press) ────────────────────────

#[cfg(desktop)]
fn validate_css_selector(sel: &str) -> Result<String, String> {
    let s = sel.trim().to_string();
    if s.is_empty() || s.len() > 500 {
        return Err("invalid-selector".into());
    }
    if s.chars()
        .any(|c| c == '\n' || c == '\r' || c == ';' || c == '\0')
    {
        return Err("invalid-selector".into());
    }
    Ok(s)
}

#[cfg(desktop)]
fn validate_fill_text(text: &str) -> Result<String, String> {
    if text.len() > 4000 {
        return Err("text-too-long".into());
    }
    Ok(text.to_string())
}

#[cfg(desktop)]
fn validate_press_key(key: &str) -> Result<String, String> {
    const ALLOWED: &[&str] = &[
        "Enter",
        "Tab",
        "Escape",
        "Backspace",
        "Delete",
        "Space",
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "End",
    ];
    let k = key.trim();
    if ALLOWED.contains(&k) {
        return Ok(k.to_string());
    }
    if k.len() == 1 {
        return Ok(k.to_string());
    }
    Err("invalid-key".into())
}

#[cfg(desktop)]
fn parse_browser_act_json(v: serde_json::Value) -> Result<(), String> {
    if v.get("ok").and_then(|x| x.as_bool()) == Some(true) {
        return Ok(());
    }
    Err(v
        .get("error")
        .and_then(|x| x.as_str())
        .unwrap_or("act-failed")
        .to_string())
}

#[cfg(desktop)]
fn js_browser_click(selector: &str) -> Result<String, String> {
    validate_css_selector(selector)?;
    browser_scripts::js_click(selector)
}

#[cfg(desktop)]
fn js_browser_fill(selector: &str, text: &str) -> Result<String, String> {
    validate_css_selector(selector)?;
    validate_fill_text(text)?;
    browser_scripts::js_fill(selector, text)
}

#[cfg(desktop)]
fn js_browser_element_exists(selector: &str) -> Result<String, String> {
    validate_css_selector(selector)?;
    browser_scripts::js_element_exists(selector)
}

#[cfg(desktop)]
fn js_browser_press(key: &str) -> Result<String, String> {
    let k = validate_press_key(key)?;
    browser_scripts::js_press(&k)
}

/// 枚举当前页可交互元素 (按钮/链接/输入框等), 供 agent 选用 selector。
#[cfg(desktop)]
const BROWSER_LIST_INTERACTIVE_JS: &str = browser_scripts::LIST_INTERACTIVE_JS;

#[cfg(all(desktop, target_os = "linux"))]
fn cdp_running(app: &AppHandle) -> bool {
    let state = app.state::<browser_cdp::BrowserCdpState>();
    state.enabled() && tauri::async_runtime::block_on(state.is_running())
}

#[cfg(desktop)]
fn browser_eval_json(app: AppHandle, js: String) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "linux")]
    {
        if cdp_running(&app) {
            let state = app.state::<browser_cdp::BrowserCdpState>();
            return tauri::async_runtime::block_on(browser_cdp::eval_json(&state, &js));
        }
        run_on_main_thread_sync(&app, move |_app| browser_linux::eval_json(&js))
    }

    #[cfg(not(target_os = "linux"))]
    {
        use std::sync::mpsc;
        use std::time::Duration;

        let wv = app
            .get_webview(BROWSER_LABEL)
            .ok_or_else(|| "浏览器视图不存在".to_string())?;
        let (tx, rx) = mpsc::sync_channel(1);
        wv.eval_with_callback(js, move |result| {
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;
        let json_str = rx
            .recv_timeout(Duration::from_secs(8))
            .map_err(|_| "执行超时".to_string())?;
        serde_json::from_str(&json_str).map_err(|e| format!("解析脚本结果失败: {e}"))
    }
}

#[cfg(desktop)]
fn browser_run_js(app: AppHandle, js: String) -> Result<(), String> {
    browser_eval_json(app, js).and_then(parse_browser_act_json)
}

/// 可交互元素 (browser.listInteractive 单项)。
#[cfg(desktop)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserInteractiveElement {
    #[serde(rename = "ref")]
    ref_id: u32,
    role: String,
    name: String,
    selector: String,
    tag: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    element_type: String,
}

/// browser.listInteractive 返回体。
#[cfg(desktop)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserInteractiveResult {
    url: String,
    title: String,
    elements: Vec<BrowserInteractiveElement>,
}

#[cfg(desktop)]
fn parse_list_interactive(v: serde_json::Value) -> Result<BrowserInteractiveResult, String> {
    serde_json::from_value(v).map_err(|e| format!("解析可交互元素失败: {e}"))
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn open_browser_view_impl(app: &AppHandle, url: String, b: Bounds) -> Result<(), String> {
    let target = if url.trim().is_empty() {
        "https://www.google.com".to_string()
    } else {
        url
    };
    let parsed = parse_http_url(&target)?;
    let _b = sanitize_browser_bounds(app, b)?;
    let window = main_window(app)?;
    if let Some(w) = app.get_webview(BROWSER_LABEL) {
        let _ = w.close();
    }
    let app_nav = app.clone();
    let app_load = app.clone();
    let builder = browser_webview_builder(BROWSER_LABEL, parsed, app_nav, app_load);
    window
        .add_child(
            builder,
            LogicalPosition::new(-10000.0, -10000.0),
            LogicalSize::new(1.0, 1.0),
        )
        .map_err(|e| e.to_string())?;
    if let Some(wv) = app.get_webview(BROWSER_LABEL) {
        let _ = wv.set_auto_resize(false);
        stash_browser_webview(&wv);
    }
    Ok(())
}

/// 打开 (或重建) 内嵌浏览器子 webview, 加载 url, 定位到主窗口内容区 bounds。
///
/// 必须为 async 命令: Windows 上 add_child 创建 WebView2 子窗口需创建它的 UI(主)线程持续泵消息才能完成,
/// 同步命令会阻塞该线程的消息泵 → 控制器创建回调永不触发 → add_child 死锁不返回 → 事件循环停摆 →
/// 之后所有 invoke(含窗控)挂起。async 命令在独立运行时线程执行, 主线程得以正常泵消息完成创建。
/// Linux 上 GTK/WebKit 仅允许主线程操作, 故在 async 命令内再经 run_on_main_thread_sync 派发。
/// (见 Tauri WebviewWindowBuilder 文档「Known issues」与 tauri#4121。)
#[cfg(desktop)]
#[tauri::command]
async fn open_browser_view(app: AppHandle, url: String, b: Bounds) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return run_on_main_thread_sync(&app, move |app| browser_linux::open(app, url, b));

    #[cfg(not(target_os = "linux"))]
    return open_browser_view_impl(&app, url, b);
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn browser_set_bounds_impl(app: &AppHandle, b: Bounds) -> Result<(), String> {
    let b = apply_browser_bounds(
        &app.get_webview(BROWSER_LABEL).ok_or("浏览器视图不存在")?,
        app,
        b,
    )?;
    #[cfg(target_os = "windows")]
    {
        if let Ok(mut g) = app.state::<BrowserWinState>().last_bounds.lock() {
            *g = Some(b);
        }
        if let Some(wv) = app.get_webview(BROWSER_LABEL) {
            browser_win_sync(&wv, app, b);
        }
    }
    Ok(())
}

/// 同步子 webview 矩形 (内容区随窗口缩放/侧栏折叠变化时调用)。
#[cfg(desktop)]
#[tauri::command]
fn browser_set_bounds(app: AppHandle, b: Bounds) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return run_on_main_thread_sync(&app, move |app| browser_linux::set_bounds(app, b));

    #[cfg(all(not(target_os = "linux"), target_os = "windows"))]
    return run_on_main_thread_sync(&app, move |app| browser_set_bounds_impl(app, b));

    #[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
    return browser_set_bounds_impl(&app, b);
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn browser_present_impl(app: &AppHandle, b: Bounds) -> Result<(), String> {
    let wv = app.get_webview(BROWSER_LABEL).ok_or("浏览器视图不存在")?;
    stash_browser_webview(&wv);
    let b = apply_browser_bounds(&wv, app, b)?;
    wv.show().map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        if let Ok(mut g) = app.state::<BrowserWinState>().last_bounds.lock() {
            *g = Some(b);
        }
        browser_win_sync(&wv, app, b);
        // WebView2 内部 HWND 可能晚于 show 才出现, 短延迟后再同步一次命中穿透。
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let app3 = app2.clone();
                let _ = app2.run_on_main_thread(move || {
                    browser_win_reclip(&app3);
                });
            });
        });
    }
    Ok(())
}

/// 同步 bounds 并显示子 webview (原子操作, 避免 Windows 上 show 与 set_bounds 竞态导致全窗遮挡)。
#[cfg(desktop)]
#[tauri::command]
fn browser_present(app: AppHandle, b: Bounds) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let r = run_on_main_thread_sync(&app, move |app| {
            browser_linux::set_bounds(app, b)?;
            browser_linux::show(app)
        });
        if let Err(e) = &r {
            eprintln!("[ideall] browser_present 失败: {e} (b={b:?})");
        }
        r
    }

    #[cfg(all(not(target_os = "linux"), target_os = "windows"))]
    {
        let r = run_on_main_thread_sync(&app, move |app| browser_present_impl(app, b));
        if let Err(e) = &r {
            eprintln!("[ideall] browser_present 失败: {e} (b={b:?})");
        }
        r
    }

    #[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
    {
        browser_present_impl(&app, b)
    }
}

/// 地址栏导航到新 url。
#[cfg(desktop)]
#[tauri::command]
fn browser_navigate(app: AppHandle, url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, move |app| browser_linux::navigate(app, &url))
    }

    #[cfg(not(target_os = "linux"))]
    {
        let parsed = parse_http_url(&url)?;
        app.get_webview(BROWSER_LABEL)
            .ok_or("浏览器视图不存在")?
            .navigate(parsed)
            .map_err(|e| e.to_string())
    }
}

#[cfg(desktop)]
#[tauri::command]
fn browser_back(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::back)
    }

    #[cfg(not(target_os = "linux"))]
    {
        app.get_webview(BROWSER_LABEL)
            .ok_or("浏览器视图不存在")?
            .eval("history.back()")
            .map_err(|e| e.to_string())
    }
}
#[cfg(desktop)]
#[tauri::command]
fn browser_forward(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::forward)
    }

    #[cfg(not(target_os = "linux"))]
    {
        app.get_webview(BROWSER_LABEL)
            .ok_or("浏览器视图不存在")?
            .eval("history.forward()")
            .map_err(|e| e.to_string())
    }
}
#[cfg(desktop)]
#[tauri::command]
fn browser_reload(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::reload)
    }

    #[cfg(not(target_os = "linux"))]
    {
        app.get_webview(BROWSER_LABEL)
            .ok_or("浏览器视图不存在")?
            .eval("location.reload()")
            .map_err(|e| e.to_string())
    }
}

// 隐藏/显示/关闭子 webview (标签切走 hide, 切回 show, 关标签 close)。
#[cfg(desktop)]
#[tauri::command]
fn browser_hide(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::hide)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let Some(wv) = app.get_webview(BROWSER_LABEL) else {
            return Ok(());
        };
        stash_browser_webview(&wv);
        Ok(())
    }
}
#[cfg(desktop)]
#[tauri::command]
fn browser_show(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::show)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        // 禁止无 bounds 的 show (会以错误尺寸挡全窗); 请用 browser_present。
        Ok(())
    }
}
/// 内嵌浏览器后端信息 (CDP / WebKit / WebView); 供 UI 显示模式徽标。
#[cfg(desktop)]
#[tauri::command]
async fn browser_get_backend(app: AppHandle) -> Result<BrowserBackendInfo, String> {
    #[cfg(target_os = "linux")]
    {
        let state = app.state::<browser_cdp::BrowserCdpState>();
        Ok(state.backend_info().await)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(BrowserBackendInfo {
            mode: "webview".into(),
            cdp_available: false,
            running: app.get_webview(BROWSER_LABEL).is_some(),
            chrome_path: None,
        })
    }
}

/// 读取内嵌浏览器当前页 (URL + 标题 + 正文); 仅桌面、浏览器标签已打开时可用。
#[cfg(desktop)]
#[tauri::command]
fn browser_get_content(app: AppHandle) -> Result<BrowserPageContent, String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::get_content)
    }

    #[cfg(not(target_os = "linux"))]
    {
        use std::sync::mpsc;
        use std::time::Duration;

        let wv = app
            .get_webview(BROWSER_LABEL)
            .ok_or_else(|| "浏览器视图不存在".to_string())?;
        let url = wv.url().map_err(|e| e.to_string())?.to_string();
        let (tx, rx) = mpsc::sync_channel(1);
        wv.eval_with_callback(BROWSER_CONTENT_JS, move |result| {
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;
        let json_str = rx
            .recv_timeout(Duration::from_secs(8))
            .map_err(|_| "读取页面超时".to_string())?;
        parse_browser_page_json(url, &json_str)
    }
}

/// 点击内嵌浏览器页面元素 (CSS 选择器)。
#[cfg(desktop)]
#[tauri::command]
fn browser_click(app: AppHandle, selector: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if cdp_running(&app) {
        let state = app.state::<browser_cdp::BrowserCdpState>();
        return tauri::async_runtime::block_on(browser_cdp::click(&state, &selector));
    }
    browser_run_js(app, js_browser_click(&selector)?)
}

/// 向内嵌浏览器输入框/文本区填写内容 (CSS 选择器 + 文本)。
#[cfg(desktop)]
#[tauri::command]
fn browser_fill(app: AppHandle, selector: String, text: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if cdp_running(&app) {
        let state = app.state::<browser_cdp::BrowserCdpState>();
        return tauri::async_runtime::block_on(browser_cdp::fill(&state, &selector, &text));
    }
    browser_run_js(app, js_browser_fill(&selector, &text)?)
}

/// 向内嵌浏览器当前焦点元素发送按键 (Enter / Tab / 单字符等)。
#[cfg(desktop)]
#[tauri::command]
fn browser_press(app: AppHandle, key: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if cdp_running(&app) {
        let state = app.state::<browser_cdp::BrowserCdpState>();
        return tauri::async_runtime::block_on(browser_cdp::press(&state, &key));
    }
    browser_run_js(app, js_browser_press(&key)?)
}

/// 列出内嵌浏览器当前页可交互元素 (按钮/链接/输入框 + 建议 selector)。
#[cfg(desktop)]
#[tauri::command]
fn browser_list_interactive(app: AppHandle) -> Result<BrowserInteractiveResult, String> {
    #[cfg(target_os = "linux")]
    if cdp_running(&app) {
        let state = app.state::<browser_cdp::BrowserCdpState>();
        return tauri::async_runtime::block_on(browser_cdp::list_interactive(&state));
    }
    let v = browser_eval_json(app, BROWSER_LIST_INTERACTIVE_JS.to_string())?;
    parse_list_interactive(v)
}

/// 等待若干毫秒 (操作后让页面加载/渲染; agent observe-act 环用)。
#[cfg(desktop)]
#[tauri::command]
async fn browser_wait(ms: u64) -> Result<(), String> {
    let ms = ms.clamp(100, 15_000);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    Ok(())
}

/// 等待内嵌浏览器页面出现匹配选择器的元素 (轮询, 超时失败)。
#[cfg(desktop)]
#[tauri::command]
async fn browser_wait_for_selector(
    app: AppHandle,
    selector: String,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let timeout = timeout_ms.unwrap_or(8000).clamp(500, 30_000);
    #[cfg(target_os = "linux")]
    if cdp_running(&app) {
        let state = app.state::<browser_cdp::BrowserCdpState>();
        return browser_cdp::wait_for_selector(&state, &selector, timeout).await;
    }
    let js = js_browser_element_exists(&selector)?;
    let start = std::time::Instant::now();
    loop {
        if browser_eval_json(app.clone(), js.clone())?
            .get("ok")
            .and_then(|x| x.as_bool())
            == Some(true)
        {
            return Ok(());
        }
        if start.elapsed().as_millis() >= timeout as u128 {
            return Err("wait-timeout".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn browser_close_impl(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview(BROWSER_LABEL) {
        #[cfg(target_os = "windows")]
        browser_win_clear_subclass(app);
        stash_browser_webview(&w);
        let _ = w.close();
    }
    if app.get_webview(BROWSER_LABEL).is_some() {
        if let Some(w) = app.get_webview(BROWSER_LABEL) {
            stash_browser_webview(&w);
            let _ = w.close();
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(mut g) = app.state::<BrowserWinState>().last_bounds.lock() {
            *g = None;
        }
        win_raise_main_webview(app);
    }
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn browser_close(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_on_main_thread_sync(&app, browser_linux::close)
    }

    #[cfg(all(not(target_os = "linux"), target_os = "windows"))]
    {
        run_on_main_thread_sync(&app, browser_close_impl)
    }

    #[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
    {
        browser_close_impl(&app)
    }
}

#[cfg(desktop)]
fn main_window_conf(app: &AppHandle) -> Result<tauri::utils::config::WindowConfig, String> {
    app.config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .or_else(|| app.config().app.windows.first())
        .cloned()
        .ok_or_else(|| "主窗口配置不存在".into())
}

/// 窗控最小化 (经自定义命令, 避免前端动态 import @tauri-apps/api/window 在 HMR 下失效)。
#[cfg(desktop)]
#[tauri::command]
fn window_minimize(app: AppHandle) -> Result<(), String> {
    main_window(&app)?.minimize().map_err(|e| e.to_string())
}

/// 窗控关闭。
#[cfg(desktop)]
#[tauri::command]
fn window_close(app: AppHandle) -> Result<(), String> {
    main_window(&app)?.close().map_err(|e| e.to_string())
}

/// 窗控最大化/还原 (WSL 下铺满主屏 work area, 非 WSL 走系统 toggleMaximize)。
#[cfg(desktop)]
#[tauri::command]
fn window_toggle_maximize(app: AppHandle) -> Result<bool, String> {
    let window = main_window(&app)?;
    let conf = main_window_conf(&app)?;
    window_placement::toggle_primary_maximize(&window, &conf)
}

#[cfg(desktop)]
#[tauri::command]
fn window_query_maximized(app: AppHandle) -> Result<bool, String> {
    let window = main_window(&app)?;
    Ok(window_placement::is_primary_maximized(&window))
}

// ── 文件 + 引擎独立窗口 ──────────────────────────────────────────────────────

#[cfg(desktop)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenEngineWindowResult {
    label: String,
    url: String,
}

#[cfg(desktop)]
fn valid_engine_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

#[cfg(desktop)]
fn valid_engine_window_path(path: &str) -> bool {
    if path.len() < 2 || path.len() > 256 || path == "/auth" || path.starts_with("/auth/") {
        return false;
    }
    path.strip_prefix('/').is_some_and(|rest| {
        rest.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        })
    })
}

#[cfg(desktop)]
fn valid_engine_window_label(label: &str) -> bool {
    let Some(rest) = label.strip_prefix("engine-") else {
        return false;
    };
    let Some((target_hash, nonce)) = rest.split_once('-') else {
        return false;
    };
    target_hash.len() == 7
        && target_hash
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && nonce.len() == 32
        && nonce
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// 二次校验前端构造的内部深链。只接受 pathname + 唯一的 file/engine/display 参数，
/// 从命令边界杜绝 scheme/host 注入、query smuggling 与认证页复用。
#[cfg(desktop)]
fn validate_engine_window_url(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 8_192
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.chars().any(char::is_control)
        || value.contains('\\')
        || value.contains('#')
    {
        return Err("invalid-engine-window-url".into());
    }

    let base = tauri::Url::parse("https://ideall.invalid/")
        .map_err(|_| "invalid-engine-window-url".to_string())?;
    let parsed = base
        .join(value)
        .map_err(|_| "invalid-engine-window-url".to_string())?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("ideall.invalid")
        || parsed.fragment().is_some()
        || !valid_engine_window_path(parsed.path())
    {
        return Err("invalid-engine-window-url".into());
    }

    let mut file_key: Option<String> = None;
    let mut engine_id: Option<String> = None;
    let mut display: Option<String> = None;
    let mut count = 0usize;
    for (key, value) in parsed.query_pairs() {
        count += 1;
        let slot = match key.as_ref() {
            "file" => &mut file_key,
            "engine" => &mut engine_id,
            "display" => &mut display,
            _ => return Err("invalid-engine-window-url".into()),
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err("invalid-engine-window-url".into());
        }
    }

    let file_key = file_key.ok_or_else(|| "invalid-engine-window-url".to_string())?;
    let engine_id = engine_id.ok_or_else(|| "invalid-engine-window-url".to_string())?;
    if count != 3
        || file_key.trim().is_empty()
        || file_key.chars().count() > 2_048
        || file_key.chars().any(|c| c.is_control() || c == '\u{fffd}')
        || !valid_engine_id(&engine_id)
        || display.as_deref() != Some("window")
    {
        return Err("invalid-engine-window-url".into());
    }
    Ok(())
}

#[cfg(desktop)]
fn allow_engine_window_navigation(url: &tauri::Url) -> bool {
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"))
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("localhost")
            && url.port() == Some(5020))
}

/// 创建不继承主窗口 capability 的独立引擎窗口。
///
/// 命令本身只授权给 `main`；新窗口 label 固定为 `engine-*`，不匹配任何 capability，
/// 因而无法调用 secure-store/http/shell 等 IPC。窗口只准在 ideall 内部 origin 导航，
/// 且拒绝页面再用 window.open 派生不受控窗口。
#[cfg(desktop)]
#[tauri::command]
async fn open_engine_window(
    app: AppHandle,
    label: String,
    url: String,
) -> Result<OpenEngineWindowResult, String> {
    if !valid_engine_window_label(&label) {
        return Err("invalid-engine-window-label".into());
    }
    validate_engine_window_url(&url)?;
    if app.get_window(&label).is_some() {
        return Err("engine-window-label-exists".into());
    }

    let app_path = url.trim_start_matches('/').into();
    tauri::WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(app_path))
        .title("ideall")
        .inner_size(1_000.0, 720.0)
        .min_inner_size(360.0, 600.0)
        .resizable(true)
        // 独立窗口没有窗控 IPC 权限，保留系统装饰以确保始终可移动/关闭。
        .decorations(true)
        .center()
        .on_navigation(allow_engine_window_navigation)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(OpenEngineWindowResult { label, url })
}

// ── agent 出站联网守卫 (web.search / web.fetch 的 Rust 侧 SSRF 闭合) ────────────────────────────────
// JS 侧 (src/lib/web-search.ts) 只能拦 IP 字面量与已知坏名; 「公网域名 A 记录指向私网/环回/元数据」必须在
// **连接前**解析+校验+钉连。本命令: 自行解析主机→IP, 任一 IP 落非全局段即拒 (fail closed), 再用 resolve_to_addrs
// 把连接钉死到已校验 IP (reqwest 不再二次解析 → 关闭名解析 SSRF 与 DNS-rebind/TOCTOU)。不跟随重定向 (JS 逐跳
// 复检, 每跳重走本命令即重解析+钉连)。桌面与移动端通用 (agent 联网在两端都该可用)。
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// IPv4 是否落在非全局可路由段 (环回/私网/link-local/CGNAT/广播/组播/文档/0 段/协议保留)。
fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1])) // CGNAT 100.64/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 协议保留
}

/// IPv6 是否为 环回/未指定/组播/link-local/ULA, 或内嵌 (mapped/compat) 回坏 IPv4。
fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return true;
    }
    let seg = ip.segments();
    if (seg[0] & 0xffc0) == 0xfe80 {
        return true; // fe80::/10 link-local
    }
    if (seg[0] & 0xfe00) == 0xfc00 {
        return true; // fc00::/7 ULA
    }
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4); // ::ffff:a.b.c.d
    }
    if seg[..6].iter().all(|&s| s == 0) {
        // IPv4-compatible ::a.b.c.d (已废弃但仍可解析) —— 低 32 位当 v4。
        let v4 = Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            (seg[6] & 0xff) as u8,
            (seg[7] >> 8) as u8,
            (seg[7] & 0xff) as u8,
        );
        return is_blocked_v4(v4);
    }
    false
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentFetchArgs {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentFetchResult {
    status: u16,
    final_url: String,
    content_type: Option<String>,
    location: Option<String>,
    body: String,
}

/// agent 出站取数 (单跳, 不跟随重定向)。Err(reason) 与 JS WebError.reason 同名 (blocked-host/timeout/...)。
#[tauri::command]
async fn agent_guarded_fetch(args: AgentFetchArgs) -> Result<AgentFetchResult, String> {
    let parsed = tauri::Url::parse(&args.url).map_err(|_| "invalid-url".to_string())?;
    if parsed.scheme() != "https" {
        return Err("blocked-protocol".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("blocked-host".into());
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    if port != 443 {
        return Err("blocked-port".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "invalid-url".to_string())?
        .to_string();
    let host_clean = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    // 解析 + 全局性校验 (fail closed: 任一解析 IP 落非全局段即拒, 杜绝多 A 记录混入私网)。
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host_clean.as_str(), port))
        .await
        .map_err(|_| "dns-failed".to_string())?
        .collect();
    if addrs.is_empty() {
        return Err("dns-failed".into());
    }
    if addrs.iter().any(|a| is_blocked_ip(a.ip())) {
        return Err("blocked-host".into());
    }

    let timeout = std::time::Duration::from_millis(args.timeout_ms.unwrap_or(10_000));
    let max_bytes = args.max_bytes.unwrap_or(2 * 1024 * 1024);

    // 钉连到已校验 IP (resolve_to_addrs) → reqwest 不再二次解析 (关闭 rebind); 不跟随重定向 (JS 逐跳复检)。
    let client = reqwest::Client::builder()
        .resolve_to_addrs(&host_clean, &addrs)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
        .map_err(|e| format!("client-build-failed: {e}"))?;

    let method = reqwest::Method::from_bytes(args.method.as_deref().unwrap_or("GET").as_bytes())
        .map_err(|_| "bad-method".to_string())?;
    let mut req = client.request(method, parsed.as_str());
    if let Some(hs) = &args.headers {
        for (k, v) in hs {
            req = req.header(k, v);
        }
    }
    if let Some(b) = args.body {
        req = req.body(b);
    }

    let mut resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "timeout"
        } else {
            "fetch-failed"
        }
        .to_string()
    })?;

    let status = resp.status().as_u16();
    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(len) = resp.content_length() {
        if len as usize > max_bytes {
            return Err("content-too-large".into());
        }
    }

    // 流式读 + 字节上限 (reqwest 已解压 → 计的是解压后字节, 也挡解压炸弹)。整体 timeout 覆盖 body 读。
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > max_bytes {
                    return Err("content-too-large".into());
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                return Err(if e.is_timeout() {
                    "timeout"
                } else {
                    "fetch-failed"
                }
                .to_string());
            }
        }
    }

    Ok(AgentFetchResult {
        status,
        final_url,
        content_type,
        location,
        body: String::from_utf8_lossy(&buf).to_string(),
    })
}

// 桌面与移动端共用入口。移动端由 `mobile_entry_point` 暴露给 iOS/Android 宿主。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WSL2 / 无独立 GPU 的 Linux 桌面下, WebKitGTK 的 DMABUF / 加速合成渲染会初始化失败
    // (libEGL / MESA ZINK / dri2 报错), 导致窗口空白。在 webview 创建前(此处, 仍单线程)默认
    // 开启软件渲染, 免去每次手动 `WEBKIT_DISABLE_DMABUF_RENDERER=1 ... pnpm app:dev`。
    // WebProcess 在窗口加载时 fork, 继承本进程内存中的 environ, 故 set_var 对其生效。
    // 仅 Linux 生效 (WebKitGTK); macOS(WKWebView)/Windows(WebView2) 无此 webview 故跳过。
    // 已手动设置则不覆盖; dev 与生产打包均走此入口, 一处搞定。
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
        // WSLg 多屏: Wayland 下程序设窗坐标无效 (outer_position 恒为 0,0), 需 X11 才能居中到主屏。
        // 副作用: XWayland 下鼠标光标可能不可见 (见 docs); IDEALL_GDK_X11=0 可退回 Wayland。
        if std::env::var_os("GDK_BACKEND").is_none()
            && std::env::var("IDEALL_GDK_X11").ok().as_deref() != Some("0")
        {
            std::env::set_var("GDK_BACKEND", "x11");
        }
        // WSLg: 抑制 AT-SPI / dconf 启动噪音 (无 a11y bus、无系统 dconf db)。
        if std::env::var_os("NO_AT_BRIDGE").is_none() {
            std::env::set_var("NO_AT_BRIDGE", "1");
        }
        if std::env::var_os("GTK_A11Y").is_none() {
            std::env::set_var("GTK_A11Y", "none");
        }
        if std::env::var_os("GSETTINGS_BACKEND").is_none() {
            std::env::set_var("GSETTINGS_BACKEND", "memory");
        }
        // 注: 内嵌浏览器子 webview (Window::add_child) 仅支持 X11 (Wayland 下不显示)。
    }

    // Windows: 子 webview WebView2; 允许内网自签 HTTPS (inventory 等)。
    #[cfg(target_os = "windows")]
    {
        if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
            std::env::set_var(
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                "--ignore-certificate-errors",
            );
        }
    }

    let context = tauri::generate_context!();

    let builder = tauri::Builder::default().manage(capture_share::CaptureShareState::default());

    // Linux/Windows 的深链接和“打开方式”会启动第二进程；单实例插件必须是首个注册插件，
    // 将参数投递给已运行主窗口并聚焦，而不是生成两个各自持有本地状态的窗口。
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        capture_share::enqueue_args(app.clone(), &args, std::path::Path::new(&cwd));
        if let Some(window) = app.get_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));

    let builder = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        // HTTP: App 内 fetch 经 Rust 侧发出, 绕过 webview CORS (agent 直连任意 LLM 端点)。
        .plugin(tauri_plugin_http::init())
        // Shell: 本地终端执行系统命令 (仅桌面; 权限由 capability 限定)。
        .plugin(tauri_plugin_shell::init())
        // Dialog: Git 仓库授权只能由 Rust 侧原生目录选择器创建。
        .plugin(tauri_plugin_dialog::init());

    // 深链接两端通用；桌面同时初始化文件参数入口、授权文件系统和窗口位置。
    let builder = builder.setup(|app| {
        use tauri_plugin_deep_link::DeepLinkExt;

        if let Some(urls) = app.deep_link().get_current()? {
            capture_share::enqueue_deep_links(app.handle(), &urls);
        }
        let deep_link_handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
            capture_share::enqueue_deep_links(&deep_link_handle, event.urls());
        });

        // Linux 需 `update-desktop-database` (desktop-file-utils); WSL/精简环境常缺失。
        // 注册失败只影响 ideall:// 协议，不应阻断启动。
        #[cfg(any(target_os = "linux", windows))]
        if let Err(error) = app.deep_link().register_all() {
            eprintln!("[ideall] deep link register_all skipped: {error}");
        }

        #[cfg(desktop)]
        {
            let args = std::env::args().collect::<Vec<_>>();
            let cwd = std::env::current_dir().unwrap_or_default();
            capture_share::enqueue_args(app.handle().clone(), &args, &cwd);
            app.manage(guarded_fs::init_state(app.handle()));

            // 多显示器: 未在 tauri.conf 指定 x/y 时, 等窗口尺寸就绪后居中到主屏。
            let conf = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .or_else(|| app.config().app.windows.first())
                .cloned();
            if conf
                .as_ref()
                .is_some_and(|w| w.x.is_none() && w.y.is_none())
            {
                if let Some(window) = app.get_window("main") {
                    window_placement::schedule_initial_placement(&window, conf.unwrap());
                }
            }
        }
        Ok(())
    });

    #[cfg(desktop)]
    let builder = builder.on_window_event(|window, event| {
        if window.label() != "main" {
            return;
        }
        if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
            capture_share::enqueue_file_paths(window.app_handle().clone(), paths.clone(), true);
        }
    });

    // agent 出站守卫命令两端通用; 内嵌浏览器命令仅桌面 (Window::add_child = desktop + unstable feature)。
    // invoke_handler 只能设一次, 故按平台分别注册全集 / 仅 agent_guarded_fetch。
    #[cfg(desktop)]
    let builder = builder
        .manage(acp_transport::init_state())
        .manage(acp_transport::init_server_state())
        .manage(oauth_callback::init_oauth_state());

    #[cfg(all(desktop, target_os = "linux"))]
    let builder = builder.manage(browser_cdp::init_state());

    #[cfg(all(desktop, target_os = "windows"))]
    let builder = builder.manage(BrowserWinState::default());

    #[cfg(desktop)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        agent_guarded_fetch,
        open_browser_view,
        browser_set_bounds,
        browser_present,
        browser_navigate,
        browser_back,
        browser_forward,
        browser_reload,
        browser_hide,
        browser_show,
        browser_close,
        browser_get_backend,
        browser_get_content,
        browser_click,
        browser_fill,
        browser_press,
        browser_list_interactive,
        browser_wait,
        browser_wait_for_selector,
        acp_transport::acp_spawn,
        acp_transport::acp_send,
        acp_transport::acp_close,
        acp_transport::acp_listen_start,
        acp_transport::acp_listen_stop,
        acp_transport::acp_server_send,
        acp_transport::acp_server_close,
        acp_transport::acp_which,
        acp_transport::acp_script_path,
        acp_transport::acp_run_once,
        oauth_callback::oauth_callback_start,
        oauth_callback::oauth_callback_stop,
        runtime_extensions::runtime_extension_discover,
        runtime_extensions::runtime_extension_verify,
        runtime_extensions::runtime_extension_spawn,
        runtime_extensions::runtime_extension_publisher_list,
        runtime_extensions::runtime_extension_publisher_inspect,
        runtime_extensions::runtime_extension_publisher_trust,
        runtime_extensions::runtime_extension_publisher_rotation_inspect,
        runtime_extensions::runtime_extension_publisher_rotation_apply,
        runtime_extensions::runtime_extension_publisher_revoke,
        runtime_extensions::runtime_extension_revocation_import,
        runtime_extensions::runtime_extension_update_prepare,
        runtime_extensions::runtime_extension_update_apply,
        runtime_extensions::runtime_extension_update_discard,
        runtime_extensions::runtime_extension_install,
        runtime_extensions::runtime_extension_rollback,
        runtime_extensions::runtime_extension_uninstall,
        extension_registry::runtime_extension_registry_snapshot,
        extension_registry::runtime_extension_registry_refresh,
        installed_apps::list_installed_apps,
        installed_apps::launch_installed_app,
        installed_apps::read_app_icon_data_url,
        guarded_fs::guarded_fs_pick_root,
        guarded_fs::guarded_fs_grant_info,
        guarded_fs::guarded_fs_revoke_grant,
        guarded_fs::guarded_fs_stat,
        guarded_fs::guarded_fs_list,
        guarded_fs::guarded_fs_read,
        guarded_fs::guarded_fs_write_text,
        secure_store::secure_store_status,
        secure_store::secure_store_get,
        secure_store::secure_store_set,
        secure_store::secure_store_delete,
        secure_store::secure_store_self_test,
        capture_share::capture_take_pending,
        window_minimize,
        window_close,
        window_toggle_maximize,
        window_query_maximized,
        open_engine_window,
    ]);
    #[cfg(not(desktop))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        agent_guarded_fetch,
        capture_share::capture_take_pending
    ]);

    // 自动更新仅桌面: 移动端经应用商店分发。
    // 仅当 tauri.conf.json 配了 `plugins.updater` (endpoints + pubkey) 时才挂 updater 插件 ——
    // 否则其初始化会因缺配置反序列化失败 (invalid type: null) 而 panic, 致桌面生产 App 启动即崩。
    // 配置缺失时静默跳过, 待发布签名就绪 (配好 plugins.updater) 自然启用。
    #[cfg(desktop)]
    let builder = if context.config().plugins.0.contains_key("updater") {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };

    let app = builder
        .build(context)
        .expect("error while building tauri application");
    app.run(|_app, _event| {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        if let tauri::RunEvent::Opened { urls } = _event {
            capture_share::enqueue_opened_files(_app.clone(), &urls);
        }
    });
}

// agent 出站守卫的 IP 全局性判定单测 (离线, 无网络)。镜像 JS 侧 web-search.ts 的拦截集 —— 两侧任一漂移即此处/JS 测失败。
#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(s: &str) -> bool {
        is_blocked_ip(s.parse::<IpAddr>().expect("ip"))
    }

    #[test]
    fn blocks_loopback_private_linklocal_metadata_cgnat() {
        for s in [
            "127.0.0.1",
            "127.1.2.3",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // 云元数据
            "169.254.0.1",
            "0.0.0.0",
            "100.64.0.1", // CGNAT
            "255.255.255.255",
            "::1",
            "::",
            "fe80::1",          // link-local
            "fc00::1",          // ULA
            "fd12:3456::1",     // ULA
            "::ffff:127.0.0.1", // IPv4-mapped → 环回
            "::ffff:169.254.169.254",
            "::ffff:10.0.0.1",
            "::127.0.0.1", // IPv4-compatible
        ] {
            assert!(blocked(s), "应拦 {s}");
        }
    }

    #[test]
    fn allows_public() {
        for s in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:2800:220:1::1"] {
            assert!(!blocked(s), "应放行公网 {s}");
        }
    }

    #[test]
    fn blocks_172_only_in_private_band() {
        assert!(!blocked("172.15.0.1"), "172.15 不在私网段");
        assert!(blocked("172.16.0.1"));
        assert!(blocked("172.31.0.1"));
        assert!(!blocked("172.32.0.1"), "172.32 不在私网段");
    }

    #[test]
    fn validates_isolated_engine_window_target() {
        assert!(valid_engine_window_label(
            "engine-01abcde-00112233445566778899aabbccddeeff"
        ));
        assert!(validate_engine_window_url(
            "/home/notes?file=local%3Anote-1&engine=builtin.preview&display=window"
        )
        .is_ok());

        for value in [
            "https://evil.test/home?file=x&engine=preview&display=window",
            "//evil.test/home?file=x&engine=preview&display=window",
            "/auth?file=x&engine=preview&display=window",
            "/home?file=x&file=y&engine=preview&display=window",
            "/home?file=x&engine=preview&display=window&extra=1",
            "/home?file=x&engine=preview%2Fother&display=window",
            "/home?file=x&engine=preview&display=tab",
        ] {
            assert!(validate_engine_window_url(value).is_err(), "应拒绝 {value}");
        }
        assert!(!valid_engine_window_label("main"));
        assert!(!valid_engine_window_label(
            "engine-01abcde-00112233445566778899AABBCCDDEEFF"
        ));
    }
}
