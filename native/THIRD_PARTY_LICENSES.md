# Native UI dependency licenses

ideall 自身继续按 Apache-2.0 发布。原生 UI 基线明确选择下列依赖提供的
Apache-2.0 许可选项；发布包同时携带仓库根目录 `LICENSE` 与本说明。

| 组件 | 固定版本或 revision | 采用许可 |
| --- | --- | --- |
| `gpui`（桌面） | crates.io `0.2.2` | Apache-2.0 |
| `gpui-component` | crates.io `0.5.1` | Apache-2.0 |
| Zed `gpui`（移动间接依赖） | `74798c68d5c63d31e2ccca5c8f5ec0a02c90679c` | Apache-2.0 |
| `gpui-mobile` | `1d3ec2a1d14a63b74d1f4269340441d4eeada27a` | Apache-2.0（上游三选一许可中的 Apache 选项） |
| `agent-client-protocol` Rust SDK | crates.io `1.3.0`（ACP wire v1） | Apache-2.0 |
| `minisign` | crates.io `0.9.1` | MIT |
| `minisign-verify` | crates.io `0.2.5` | MIT |
| `semver` | crates.io `1.0.28` | Apache-2.0 |

Windows 构建工具锁定 WiX Toolset `4.0.6` 与 NSIS `3.12.0`。WiX 只生成 MSI，
不随应用分发运行时；NSIS 安装器 stub 采用 zlib/libpng 许可。正式发布 inventory 仍需保留
这两项构建工具及版本，以便重现安装器。

Android 的 `GpuiFilePicker`/`GpuiPickerActivity` 平台桥接基于上述固定 revision
中的同名示例适配，并同样采用其 Apache-2.0 许可选项。

完整传递依赖及其许可证仍应在正式商店发布前由自动 SBOM/许可证扫描器生成并复核。
