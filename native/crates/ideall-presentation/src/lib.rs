//! GPUI-independent presentation semantics shared by every native shell.
//!
//! Keep colors as plain RGB integers and navigation as static data. Desktop
//! and mobile deliberately use different GPUI revisions, so this crate must
//! never expose framework-specific types.

pub mod color {
    // 基础色板与桌面「清氧工作台」色板同源：移动端与桌面共享同一套
    // 方向 A 浅色语义（含浅色底栏），仅保留移动端特有的状态色。
    pub const CANVAS: u32 = desktop::CANVAS;
    pub const SURFACE: u32 = desktop::SURFACE;
    pub const SURFACE_MUTED: u32 = desktop::SURFACE_MUTED;
    pub const SURFACE_RAISED: u32 = desktop::SURFACE_RAISED;
    pub const INK: u32 = desktop::INK;
    pub const INK_MUTED: u32 = desktop::INK_MUTED;
    pub const INK_FAINT: u32 = desktop::INK_FAINT;
    pub const BORDER: u32 = desktop::BORDER;
    pub const BORDER_STRONG: u32 = desktop::BORDER_STRONG;
    pub const ACCENT: u32 = desktop::ACCENT;
    pub const ACCENT_SOFT: u32 = desktop::ACCENT_SOFT;
    pub const ACCENT_ON_RAIL: u32 = desktop::ACCENT_ON_RAIL;
    pub const RAIL: u32 = desktop::RAIL;
    pub const RAIL_MUTED: u32 = desktop::RAIL_MUTED;
    pub const RAIL_ACTIVE: u32 = desktop::RAIL_ACTIVE;
    pub const RAIL_INK: u32 = desktop::RAIL_INK;
    pub const INFO: u32 = 0x315f9f;
    pub const INFO_SOFT: u32 = 0xe8eef8;
    pub const SUCCESS: u32 = ACCENT;
    pub const SUCCESS_SOFT: u32 = ACCENT_SOFT;
    pub const WARNING: u32 = 0x8a5a13;
    pub const WARNING_SOFT: u32 = desktop::WARNING_SOFT;
    pub const DANGER: u32 = desktop::DANGER;
    pub const DANGER_SOFT: u32 = 0xffe9e7;
    pub const DANGER_BORDER: u32 = desktop::DANGER_BORDER;
    pub const DISABLED: u32 = 0xaab4c2;
    pub const MESSAGE_TOOL: u32 = desktop::MESSAGE_TOOL;

    /// 共享浅色色板（桌面与移动端同源）；桌面附加 Token 在本模块内扩展。
    pub mod desktop {
        pub const CANVAS: u32 = 0xf4f6f4;
        pub const SURFACE: u32 = 0xffffff;
        pub const SURFACE_MUTED: u32 = 0xf8faf8;
        pub const SURFACE_RAISED: u32 = 0xffffff;
        pub const INK: u32 = 0x1b211e;
        pub const INK_MUTED: u32 = 0x6a726d;
        pub const INK_FAINT: u32 = 0x9aa29c;
        pub const BORDER: u32 = 0xe4e8e5;
        pub const BORDER_STRONG: u32 = 0xcdd4cf;
        pub const ACCENT: u32 = 0x16825d;
        pub const ACCENT_HOVER: u32 = 0x126f50;
        pub const ACCENT_ACTIVE: u32 = 0x0e5d43;
        pub const ACCENT_SOFT: u32 = 0xe6f4ec;
        pub const ACCENT_ON_RAIL: u32 = ACCENT;
        pub const RAIL: u32 = 0xf7f9f6;
        pub const RAIL_MUTED: u32 = 0x6f7973;
        pub const RAIL_ACTIVE: u32 = ACCENT_SOFT;
        pub const RAIL_INK: u32 = INK;
        pub const DANGER: u32 = 0xb84943;
        pub const DANGER_BORDER: u32 = 0xddb7b3;
        pub const WARNING_SOFT: u32 = 0xfff3d6;
        pub const MESSAGE_TOOL: u32 = 0xf0f3f1;
        pub const TOOLBAR: u32 = SURFACE_MUTED;
        pub const EDITOR: u32 = 0xffffff;
        pub const EDITOR_GUTTER: u32 = 0xf7f8f6;
        pub const HOVER: u32 = 0xeff2f0;
        pub const SELECTION: u32 = 0xddf0e5;
        pub const STATUS_BAR: u32 = CANVAS;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationItem {
    pub section: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: &'static str,
}

pub const NAVIGATION: [NavigationItem; 5] = [
    NavigationItem {
        section: "我的",
        label: "工作台",
        icon: "▤",
        shortcut: "Alt 1",
    },
    NavigationItem {
        section: "活动",
        label: "最近",
        icon: "◷",
        shortcut: "Alt 2",
    },
    NavigationItem {
        section: "浏览",
        label: "浏览",
        icon: "◈",
        shortcut: "Alt 3",
    },
    NavigationItem {
        section: "应用",
        label: "Agent",
        icon: "✦",
        shortcut: "Alt 4",
    },
    NavigationItem {
        section: "设置",
        label: "设置",
        icon: "⚙",
        shortcut: "Alt 5",
    },
];

pub fn section_eyebrow(section: &str) -> &'static str {
    match section {
        "我的" => "PROJECT",
        "活动" => "RECENT FILES",
        "浏览" => "DISCOVER",
        "应用" => "AGENT TOOLS",
        "设置" => "SETTINGS",
        "搜索" => "SEARCH",
        "回收站" => "TRASH",
        _ => "IDEALL",
    }
}

pub fn section_description(section: &str) -> &'static str {
    match section {
        "我的" => "浏览本地项目、笔记与文件",
        "活动" => "最近修改的本地内容",
        "浏览" => "书签、关注源与网页",
        "应用" => "模型、ACP 与原生 Engine",
        "设置" => "同步、安全与应用更新",
        "搜索" => "跨标题与正文查找本地内容",
        "回收站" => "恢复或永久移除本地内容",
        _ => "本地优先个人信息终端",
    }
}

pub fn section_status(section: &str) -> &'static str {
    match section {
        "活动" => "按最近修改排序",
        "浏览" => "本地书签与关注源",
        "应用" => "原生 Agent 可直连模型，并通过授权 MCP 使用本地工具",
        "设置" => "同步凭据仅保存在系统安全存储中",
        "回收站" => "可恢复或永久删除",
        "收集箱" => "捕获的内容先进收集箱，稍后再归类",
        _ => "所有内容保存在本机",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestToken(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LatestRequest {
    generation: u64,
}

impl LatestRequest {
    pub fn start(&mut self) -> RequestToken {
        self.generation = self.generation.wrapping_add(1);
        RequestToken(self.generation)
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn is_current(&self, token: RequestToken) -> bool {
        token.0 == self.generation
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn primary_navigation_is_unique_and_has_complete_copy() {
        let sections = NAVIGATION
            .iter()
            .map(|item| item.section)
            .collect::<HashSet<_>>();
        assert_eq!(sections.len(), NAVIGATION.len());
        for item in NAVIGATION {
            assert!(!item.icon.is_empty());
            assert!(!item.label.is_empty());
            assert!(!item.shortcut.is_empty());
            assert_ne!(section_eyebrow(item.section), "IDEALL");
            assert_ne!(section_description(item.section), "本地优先个人信息终端");
        }
    }

    #[test]
    fn latest_request_rejects_superseded_and_invalidated_results() {
        let mut requests = LatestRequest::default();
        let first = requests.start();
        let second = requests.start();
        assert!(!requests.is_current(first));
        assert!(requests.is_current(second));

        requests.invalidate();
        assert!(!requests.is_current(second));
    }
}
