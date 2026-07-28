use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub(super) struct IdeallAssets;

const ACTIVITY_ICON_PATHS: [&str; 5] = [
    "icons/folder-open.svg",
    "icons/calendar.svg",
    "icons/globe.svg",
    "icons/bot.svg",
    "icons/settings-2.svg",
];

impl AssetSource for IdeallAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/folder-open.svg" => Some(include_bytes!("../assets/icons/folder-open.svg")),
            "icons/calendar.svg" => Some(include_bytes!("../assets/icons/calendar.svg")),
            "icons/globe.svg" => Some(include_bytes!("../assets/icons/globe.svg")),
            "icons/bot.svg" => Some(include_bytes!("../assets/icons/bot.svg")),
            "icons/settings-2.svg" => Some(include_bytes!("../assets/icons/settings-2.svg")),
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if matches!(path.trim_end_matches('/'), "icons") {
            return Ok(ACTIVITY_ICON_PATHS
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

    #[test]
    fn every_activity_icon_is_embedded() {
        for path in ACTIVITY_ICON_PATHS {
            assert!(IdeallAssets.load(path).unwrap().is_some(), "{path}");
        }
    }
}
