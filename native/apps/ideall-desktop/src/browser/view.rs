use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSurface {
    Bookmark,
    Portal,
}

pub(crate) fn browser_surface(
    active_engine_id: Option<&str>,
    selected_kind: Option<NodeKind>,
) -> Option<BrowserSurface> {
    match (active_engine_id, selected_kind) {
        (Some("ideall.browser"), Some(NodeKind::Bookmark)) => Some(BrowserSurface::Bookmark),
        (Some("ideall.info" | "ideall.community"), None) => Some(BrowserSurface::Portal),
        _ => None,
    }
}

impl IdeallDesktop {
    pub(crate) fn render_browser(
        &self,
        surface: BrowserSurface,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let embedded_browser = self.browser.webview();
        let current_url = self.browser.current_url().unwrap_or_default().to_owned();
        let title = match surface {
            BrowserSurface::Bookmark => self.editor.title_input.read(cx).value().to_string(),
            BrowserSurface::Portal => match self.editor.active_engine_id.as_deref() {
                Some("ideall.info") => "资讯".into(),
                Some("ideall.community") => "社区".into(),
                _ => "浏览器".into(),
            },
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(EDITOR))
            .child(
                div()
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(
                        Button::new("browser-back")
                            .icon(IconName::ArrowLeft)
                            .tooltip("后退")
                            .small()
                            .ghost()
                            .disabled(!self.browser.can_go_back())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate_browser_back(window, cx);
                            })),
                    )
                    .child(
                        Button::new("browser-forward")
                            .icon(IconName::ArrowRight)
                            .tooltip("前进")
                            .small()
                            .ghost()
                            .disabled(!self.browser.can_go_forward())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate_browser_forward(window, cx);
                            })),
                    )
                    .child(
                        Button::new("browser-reload")
                            .icon(IconName::Redo)
                            .tooltip("重新加载")
                            .small()
                            .ghost()
                            .disabled(current_url.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reload_browser(window, cx);
                            })),
                    )
                    .child(
                        Input::new(self.browser.address_input())
                            .w_full()
                            .small()
                            .prefix(
                                div().pl_1().child(
                                    Icon::new(IconName::Globe)
                                        .size(px(14.0))
                                        .text_color(rgb(INK_FAINT)),
                                ),
                            ),
                    )
                    .child(
                        Button::new("browser-open-external")
                            .label("外部打开")
                            .icon(IconName::ExternalLink)
                            .small()
                            .ghost()
                            .disabled(current_url.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_current_browser_external(cx);
                            })),
                    )
                    .when(surface == BrowserSurface::Bookmark, |toolbar| {
                        toolbar.child(
                            Button::new("browser-edit-bookmark")
                                .label("编辑书签")
                                .icon(IconName::BookOpen)
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_engine_preference(
                                        "ideall.bookmark",
                                        window,
                                        cx,
                                    );
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE_MUTED))
                    .child(
                        div()
                            .px_2()
                            .py(px(3.0))
                            .rounded_md()
                            .bg(rgb(ACCENT_SOFT))
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child("BROWSER"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_sm()
                            .text_color(rgb(INK))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(INK_FAINT))
                            .child("隔离网页内容"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .bg(rgb(EDITOR))
                    .when(embedded_browser.is_some(), |body| {
                        body.child(
                            embedded_browser
                                .clone()
                                .expect("browser WebView is present when rendered"),
                        )
                    })
                    .when(embedded_browser.is_none(), |body| {
                        body.flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .max_w(px(560.0))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap_3()
                                    .p_8()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .bg(rgb(SURFACE))
                                    .child(
                                        div()
                                            .text_xl()
                                            .text_color(rgb(INK))
                                            .child("网页已交给系统浏览器"),
                                    )
                                    .child(
                                        div()
                                            .text_center()
                                            .text_sm()
                                            .text_color(rgb(INK_MUTED))
                                            .child(
                                                "当前平台没有可靠的 GPUI 子窗口 WebView；本地文件和凭据不会注入网页。",
                                            ),
                                    )
                                    .child(
                                        Button::new("browser-open-external-fallback")
                                            .label("再次在系统浏览器中打开")
                                            .primary()
                                            .disabled(current_url.is_empty())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_current_browser_external(cx);
                                            })),
                                    ),
                            )
                    }),
            )
            .child(
                div()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .text_xs()
                    .text_color(rgb(INK_MUTED))
                    .child(
                        self.status
                            .clone()
                            .unwrap_or_else(|| "Browser 已就绪".into()),
                    )
                    .child(div().flex_1())
                    .child("HTTP(S)")
                    .child("无本地文件权限")
                    .child("无密钥权限"),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_routing_keeps_browser_out_of_the_document_editor() {
        assert_eq!(
            browser_surface(Some("ideall.browser"), Some(NodeKind::Bookmark)),
            Some(BrowserSurface::Bookmark)
        );
        assert_eq!(
            browser_surface(Some("ideall.browser"), Some(NodeKind::Note)),
            None
        );
        assert_eq!(
            browser_surface(Some("ideall.info"), None),
            Some(BrowserSurface::Portal)
        );
        assert_eq!(
            browser_surface(Some("ideall.community"), None),
            Some(BrowserSurface::Portal)
        );
        assert_eq!(browser_surface(Some("ideall.note"), None), None);
    }
}
