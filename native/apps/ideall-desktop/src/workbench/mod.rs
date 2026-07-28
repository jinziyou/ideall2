mod actions;
mod view;

pub(crate) fn init(cx: &mut gpui::App) {
    actions::init(cx);
}
