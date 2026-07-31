pub(crate) mod actions;
mod navigation;
mod view;

pub(crate) fn init(cx: &mut gpui::App) {
    actions::init(cx);
}
