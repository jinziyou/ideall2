use super::*;

#[derive(Clone)]
pub(crate) struct DraggedNode {
    pub(crate) id: String,
    pub(crate) title: String,
}

impl Render for DraggedNode {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(260.0))
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER_STRONG))
            .bg(rgb(SURFACE_RAISED))
            .text_sm()
            .text_color(rgb(INK))
            .child(self.title.clone())
    }
}

pub(crate) fn kind_icon(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "◆",
        NodeKind::Note => "N",
        NodeKind::Bookmark => "↗",
        NodeKind::File => "F",
        NodeKind::Feed => "◎",
        NodeKind::Thread => "#",
    }
}

pub(crate) fn kind_color(kind: NodeKind) -> u32 {
    match kind {
        NodeKind::Folder => 0xb47b18,
        NodeKind::Note => 0x3574f0,
        NodeKind::Bookmark => 0x478c6c,
        NodeKind::File => 0x7656c9,
        NodeKind::Feed => 0xc76834,
        NodeKind::Thread => 0x9b4f78,
    }
}
