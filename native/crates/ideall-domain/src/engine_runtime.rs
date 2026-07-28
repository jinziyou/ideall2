use ideall_protocol::EngineAccess;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnginePlatform {
    LinuxDesktop,
    MacosDesktop,
    WindowsDesktop,
    Android,
    Ios,
    DesktopPreview,
    MobilePreview,
}

impl EnginePlatform {
    pub const fn current_desktop() -> Self {
        if cfg!(target_os = "linux") {
            Self::LinuxDesktop
        } else if cfg!(target_os = "macos") {
            Self::MacosDesktop
        } else if cfg!(target_os = "windows") {
            Self::WindowsDesktop
        } else {
            Self::DesktopPreview
        }
    }

    pub const fn current_mobile() -> Self {
        if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else {
            Self::MobilePreview
        }
    }

    const fn is_mobile(self) -> bool {
        matches!(self, Self::Android | Self::Ios | Self::MobilePreview)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineRuntimeKind {
    Native,
    EmbeddedWeb,
    SystemExternal,
    MetadataOnly,
    Unavailable,
}

impl EngineRuntimeKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Native => "原生",
            Self::EmbeddedWeb => "隔离 WebView",
            Self::SystemExternal => "系统应用交接",
            Self::MetadataOnly => "仅元数据",
            Self::Unavailable => "不可用",
        }
    }

    pub const fn is_available(self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineRuntimeCapability {
    pub engine_id: &'static str,
    pub label: &'static str,
    pub kind: EngineRuntimeKind,
    pub access: EngineAccess,
    pub detail: &'static str,
}

pub fn engine_runtime_capabilities(platform: EnginePlatform) -> Vec<EngineRuntimeCapability> {
    let native = EngineRuntimeKind::Native;
    let read_write = EngineAccess::ReadWrite;
    let read_only = EngineAccess::ReadOnly;
    let web_kind = match platform {
        EnginePlatform::LinuxDesktop => EngineRuntimeKind::SystemExternal,
        EnginePlatform::MacosDesktop
        | EnginePlatform::WindowsDesktop
        | EnginePlatform::Android
        | EnginePlatform::Ios => EngineRuntimeKind::EmbeddedWeb,
        EnginePlatform::DesktopPreview | EnginePlatform::MobilePreview => {
            EngineRuntimeKind::Unavailable
        }
    };
    let web_detail = match web_kind {
        EngineRuntimeKind::EmbeddedWeb => "隔离系统 WebView；不向远端注入本地文件或密钥能力",
        EngineRuntimeKind::SystemExternal => "当前 GPUI 后端没有可靠子窗口 WebView，交给系统浏览器",
        _ => "仅在受支持的桌面或移动目标上启用 WebView",
    };
    let professional_file_kind = if platform.is_mobile() {
        EngineRuntimeKind::MetadataOnly
    } else {
        EngineRuntimeKind::SystemExternal
    };
    let professional_file_detail = if platform.is_mobile() {
        "显示文件类型、大小与 Blob 身份；移动文件交接尚未提供安全 URI bridge"
    } else {
        "以只读临时副本交给用户选择的系统应用；ideall 不解释或修改专业格式"
    };

    vec![
        capability(
            "ideall.note",
            "页面",
            native,
            read_write,
            "Plate 常用块可逆编辑，未知块无损保护",
        ),
        capability(
            "ideall.bookmark",
            "书签",
            native,
            read_write,
            "本地编辑、系统浏览器打开与安全 Web 入口",
        ),
        capability(
            "ideall.feed",
            "关注",
            native,
            read_only,
            "本地关注源与同步投影",
        ),
        capability(
            "ideall.thread",
            "对话",
            native,
            read_write,
            "本地持久线程、BYOK 工具循环与审计",
        ),
        capability(
            "ideall.directory",
            "文件树",
            native,
            read_only,
            "本地层级、搜索、回收站与恢复",
        ),
        capability(
            "ideall.preview",
            "通用预览",
            native,
            read_only,
            "Markdown 预览；其他格式显示安全元数据",
        ),
        capability(
            "ideall.code",
            "开发",
            native,
            read_write,
            if platform.is_mobile() {
                "移动文本编辑；不宣称桌面级语法服务"
            } else {
                "GPUI code editor 与按扩展名语法高亮"
            },
        ),
        capability("ideall.browser", "浏览器", web_kind, read_only, web_detail),
        capability("ideall.info", "资讯", web_kind, read_only, web_detail),
        capability("ideall.community", "社区", web_kind, read_only, web_detail),
        capability(
            "ideall.audio",
            "音频",
            professional_file_kind,
            read_only,
            professional_file_detail,
        ),
        capability(
            "ideall.database",
            "数据库",
            professional_file_kind,
            read_only,
            professional_file_detail,
        ),
        capability(
            "ideall.git",
            "Git",
            EngineRuntimeKind::Unavailable,
            read_only,
            "未提供经目录授权的仓库 provider；不会对任意路径调用 git",
        ),
        capability(
            "ideall.shell",
            "终端",
            EngineRuntimeKind::Unavailable,
            read_only,
            "未提供跨平台 PTY、进程隔离和命令审计；不会降级为无约束 shell",
        ),
        capability(
            "ideall.extensions",
            "扩展",
            if platform.is_mobile() {
                EngineRuntimeKind::Unavailable
            } else {
                native
            },
            read_only,
            if platform.is_mobile() {
                "移动平台不启动外部进程；可继续使用内置 BYOK Agent"
            } else {
                "官方 ACP v1 客户端；仅显式 argv，权限请求默认拒绝"
            },
        ),
    ]
}

fn capability(
    engine_id: &'static str,
    label: &'static str,
    kind: EngineRuntimeKind,
    access: EngineAccess,
    detail: &'static str,
) -> EngineRuntimeCapability {
    EngineRuntimeCapability {
        engine_id,
        label,
        kind,
        access,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(platform: EnginePlatform, engine_id: &str) -> EngineRuntimeCapability {
        engine_runtime_capabilities(platform)
            .into_iter()
            .find(|capability| capability.engine_id == engine_id)
            .unwrap_or_else(|| panic!("missing runtime capability {engine_id}"))
    }

    #[test]
    fn every_professional_engine_has_an_explicit_runtime_state() {
        for platform in [
            EnginePlatform::LinuxDesktop,
            EnginePlatform::MacosDesktop,
            EnginePlatform::WindowsDesktop,
            EnginePlatform::Android,
            EnginePlatform::Ios,
        ] {
            for engine_id in [
                "ideall.info",
                "ideall.community",
                "ideall.browser",
                "ideall.audio",
                "ideall.code",
                "ideall.git",
                "ideall.shell",
                "ideall.database",
                "ideall.extensions",
            ] {
                let capability = find(platform, engine_id);
                assert!(!capability.detail.is_empty());
            }
        }
    }

    #[test]
    fn unsupported_process_engines_are_not_claimed_as_native() {
        for platform in [EnginePlatform::LinuxDesktop, EnginePlatform::Android] {
            for engine_id in ["ideall.git", "ideall.shell"] {
                let capability = find(platform, engine_id);
                assert_eq!(capability.kind, EngineRuntimeKind::Unavailable);
                assert_eq!(capability.access, EngineAccess::ReadOnly);
            }
        }
    }

    #[test]
    fn platform_web_and_professional_file_downgrades_are_precise() {
        assert_eq!(
            find(EnginePlatform::LinuxDesktop, "ideall.info").kind,
            EngineRuntimeKind::SystemExternal
        );
        assert_eq!(
            find(EnginePlatform::MacosDesktop, "ideall.info").kind,
            EngineRuntimeKind::EmbeddedWeb
        );
        assert_eq!(
            find(EnginePlatform::Android, "ideall.audio").kind,
            EngineRuntimeKind::MetadataOnly
        );
        assert_eq!(
            find(EnginePlatform::WindowsDesktop, "ideall.database").kind,
            EngineRuntimeKind::SystemExternal
        );
    }
}
