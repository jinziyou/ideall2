//! GPUI-independent presentation semantics shared by every native shell.
//!
//! Keep colors as plain RGB integers and navigation as static data. Desktop
//! and mobile deliberately use different GPUI revisions, so this crate must
//! never expose framework-specific types.

pub mod color {
    pub const CANVAS: u32 = 0xf4f1ea;
    pub const SURFACE: u32 = 0xfffefa;
    pub const SURFACE_MUTED: u32 = 0xf8f6f0;
    pub const SURFACE_RAISED: u32 = 0xffffff;
    pub const INK: u32 = 0x17211e;
    pub const INK_MUTED: u32 = 0x65716c;
    pub const INK_FAINT: u32 = 0x8b948f;
    pub const BORDER: u32 = 0xdedfd8;
    pub const BORDER_STRONG: u32 = 0xc9cec7;
    pub const ACCENT: u32 = 0x176b55;
    pub const ACCENT_SOFT: u32 = 0xdcece4;
    pub const ACCENT_ON_RAIL: u32 = 0x8ed6b6;
    pub const RAIL: u32 = 0x13211d;
    pub const RAIL_MUTED: u32 = 0x9bb0a8;
    pub const RAIL_ACTIVE: u32 = 0x28463c;
    pub const RAIL_INK: u32 = 0xf3f6f2;
    pub const INFO: u32 = 0x315f9f;
    pub const INFO_SOFT: u32 = 0xe8eef8;
    pub const SUCCESS: u32 = ACCENT;
    pub const SUCCESS_SOFT: u32 = ACCENT_SOFT;
    pub const WARNING: u32 = 0x8a5a13;
    pub const WARNING_SOFT: u32 = 0xfff2cf;
    pub const DANGER: u32 = 0xa03b35;
    pub const DANGER_SOFT: u32 = 0xffe9e7;
    pub const DANGER_BORDER: u32 = 0xe3c1b6;
    pub const DISABLED: u32 = 0xaab4c2;
    pub const MESSAGE_TOOL: u32 = 0xf0f2ef;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationItem {
    pub section: &'static str,
    pub icon: &'static str,
}

pub const NAVIGATION: [NavigationItem; 5] = [
    NavigationItem {
        section: "我的",
        icon: "⌂",
    },
    NavigationItem {
        section: "活动",
        icon: "◷",
    },
    NavigationItem {
        section: "浏览",
        icon: "◇",
    },
    NavigationItem {
        section: "应用",
        icon: "✦",
    },
    NavigationItem {
        section: "设置",
        icon: "⚙",
    },
];

pub fn section_eyebrow(section: &str) -> &'static str {
    match section {
        "我的" => "LOCAL LIBRARY",
        "活动" => "LOCAL ACTIVITY",
        "浏览" => "DISCOVER",
        "应用" => "NATIVE CAPABILITIES",
        "设置" => "SYSTEM",
        "搜索" => "SEARCH",
        "回收站" => "TRASH",
        _ => "IDEALL",
    }
}

pub fn section_description(section: &str) -> &'static str {
    match section {
        "我的" => "笔记、文件与收藏都在设备上",
        "活动" => "回到最近处理过的信息",
        "浏览" => "连接网页、发布者与外部世界",
        "应用" => "让 Agent 和 Engine 在授权边界内工作",
        "设置" => "管理同步、安全与原生应用",
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
