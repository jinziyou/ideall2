use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;

pub(super) struct IdeallAssets;

#[derive(Clone, Copy)]
pub(super) enum IdeallIcon {
    Brand,
    ShieldCheck,
    Wrench,
}

impl IconNamed for IdeallIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Brand => "icons/brand.svg",
            Self::ShieldCheck => "icons/shield-check.svg",
            Self::Wrench => "icons/wrench.svg",
        }
        .into()
    }
}

const ICON_PATHS: [&str; 29] = [
    "icons/arrow-left.svg",
    "icons/arrow-right.svg",
    "icons/arrow-up.svg",
    "icons/book-open.svg",
    "icons/bot.svg",
    "icons/brand.svg",
    "icons/calendar.svg",
    "icons/chevron-down.svg",
    "icons/chevron-right.svg",
    "icons/circle-user.svg",
    "icons/delete.svg",
    "icons/external-link.svg",
    "icons/eye-off.svg",
    "icons/eye.svg",
    "icons/file.svg",
    "icons/folder-closed.svg",
    "icons/folder-open.svg",
    "icons/gallery-vertical-end.svg",
    "icons/globe.svg",
    "icons/layout-dashboard.svg",
    "icons/panel-left.svg",
    "icons/panel-right-close.svg",
    "icons/panel-right-open.svg",
    "icons/plus.svg",
    "icons/redo.svg",
    "icons/search.svg",
    "icons/settings-2.svg",
    "icons/shield-check.svg",
    "icons/wrench.svg",
];

impl AssetSource for IdeallAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/arrow-left.svg" => Some(include_bytes!("../assets/icons/arrow-left.svg")),
            "icons/arrow-right.svg" => Some(include_bytes!("../assets/icons/arrow-right.svg")),
            "icons/arrow-up.svg" => Some(include_bytes!("../assets/icons/arrow-up.svg")),
            "icons/book-open.svg" => Some(include_bytes!("../assets/icons/book-open.svg")),
            "icons/brand.svg" => Some(include_bytes!("../assets/icons/brand.svg")),
            "icons/bot.svg" => Some(include_bytes!("../assets/icons/bot.svg")),
            "icons/calendar.svg" => Some(include_bytes!("../assets/icons/calendar.svg")),
            "icons/chevron-down.svg" => Some(include_bytes!("../assets/icons/chevron-down.svg")),
            "icons/chevron-right.svg" => Some(include_bytes!("../assets/icons/chevron-right.svg")),
            "icons/circle-user.svg" => Some(include_bytes!("../assets/icons/circle-user.svg")),
            "icons/delete.svg" => Some(include_bytes!("../assets/icons/delete.svg")),
            "icons/external-link.svg" => Some(include_bytes!("../assets/icons/external-link.svg")),
            "icons/eye-off.svg" => Some(include_bytes!("../assets/icons/eye-off.svg")),
            "icons/eye.svg" => Some(include_bytes!("../assets/icons/eye.svg")),
            "icons/file.svg" => Some(include_bytes!("../assets/icons/file.svg")),
            "icons/folder-closed.svg" => Some(include_bytes!("../assets/icons/folder-closed.svg")),
            "icons/folder-open.svg" => Some(include_bytes!("../assets/icons/folder-open.svg")),
            "icons/gallery-vertical-end.svg" => {
                Some(include_bytes!("../assets/icons/gallery-vertical-end.svg"))
            }
            "icons/globe.svg" => Some(include_bytes!("../assets/icons/globe.svg")),
            "icons/layout-dashboard.svg" => {
                Some(include_bytes!("../assets/icons/layout-dashboard.svg"))
            }
            "icons/panel-left.svg" => Some(include_bytes!("../assets/icons/panel-left.svg")),
            "icons/panel-right-close.svg" => {
                Some(include_bytes!("../assets/icons/panel-right-close.svg"))
            }
            "icons/panel-right-open.svg" => {
                Some(include_bytes!("../assets/icons/panel-right-open.svg"))
            }
            "icons/plus.svg" => Some(include_bytes!("../assets/icons/plus.svg")),
            "icons/redo.svg" => Some(include_bytes!("../assets/icons/redo.svg")),
            "icons/search.svg" => Some(include_bytes!("../assets/icons/search.svg")),
            "icons/settings-2.svg" => Some(include_bytes!("../assets/icons/settings-2.svg")),
            "icons/shield-check.svg" => Some(include_bytes!("../assets/icons/shield-check.svg")),
            "icons/wrench.svg" => Some(include_bytes!("../assets/icons/wrench.svg")),
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if matches!(path.trim_end_matches('/'), "icons") {
            return Ok(ICON_PATHS
                .iter()
                .map(|path| SharedString::from(path.rsplit('/').next().unwrap_or(path)))
                .collect());
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::IconName;

    #[test]
    fn every_desktop_icon_is_embedded() {
        let used_icons = [
            IconName::ArrowLeft,
            IconName::ArrowRight,
            IconName::ArrowUp,
            IconName::BookOpen,
            IconName::Bot,
            IconName::Calendar,
            IconName::ChevronDown,
            IconName::ChevronRight,
            IconName::CircleUser,
            IconName::Delete,
            IconName::ExternalLink,
            IconName::Eye,
            IconName::EyeOff,
            IconName::File,
            IconName::FolderClosed,
            IconName::FolderOpen,
            IconName::GalleryVerticalEnd,
            IconName::Globe,
            IconName::LayoutDashboard,
            IconName::PanelLeft,
            IconName::PanelRightClose,
            IconName::PanelRightOpen,
            IconName::Plus,
            IconName::Redo,
            IconName::Search,
            IconName::Settings2,
        ];
        for path in used_icons.into_iter().map(IconNamed::path) {
            assert!(
                IdeallAssets.load(path.as_ref()).unwrap().is_some(),
                "{path}"
            );
        }
        assert!(
            IdeallAssets
                .load(IdeallIcon::Brand.path().as_ref())
                .unwrap()
                .is_some()
        );
        for icon in [IdeallIcon::ShieldCheck, IdeallIcon::Wrench] {
            assert!(IdeallAssets.load(icon.path().as_ref()).unwrap().is_some());
        }
    }
}
