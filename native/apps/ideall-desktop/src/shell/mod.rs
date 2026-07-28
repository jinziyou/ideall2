use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

mod actions;
mod command;

use command::{ShellCommandSpec, execute_shell_command, program_exists};

/// UI state and process lifecycle for the future Shell surface.
///
/// The component is intentionally not mounted in the desktop workbench yet.
/// Keeping its inputs and subscriptions here prevents the application shell
/// from acquiring terminal-specific state before the capability is exposed.
pub(super) struct ShellWorkspace {
    program_input: Entity<InputState>,
    args_input: Entity<InputState>,
    cwd_input: Entity<InputState>,
    output_input: Entity<InputState>,
    running: bool,
    run_count: u64,
    last_status: SharedString,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
}

impl ShellWorkspace {
    pub(super) fn new(window: &mut Window, cx: &mut Context<IdeallDesktop>) -> Self {
        let program_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("程序，例如 cargo、git、python"));
        let args_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("argv 参数，不经过 shell 展开"));
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|path| path.to_str().map(str::to_owned))
            .unwrap_or_else(|| "/".into());
        let cwd_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(cwd)
                .placeholder("绝对工作目录")
        });
        let output_input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("bash")
                .searchable(true)
                .default_value("等待执行命令。")
        });

        Self {
            program_input,
            args_input,
            cwd_input,
            output_input,
            running: false,
            run_count: 0,
            last_status: "就绪".into(),
            cancel: None,
            task: None,
        }
    }

    pub(super) fn subscribe(
        &self,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> Subscription {
        cx.subscribe_in(
            &self.args_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.run_shell_command(window, cx);
                }
            },
        )
    }
}

impl Drop for ShellWorkspace {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
