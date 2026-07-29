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
