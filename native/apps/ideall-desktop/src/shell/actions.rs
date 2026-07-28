use super::*;

impl IdeallDesktop {
    pub(crate) fn run_shell_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shell.running {
            return;
        }
        let program = self.shell.program_input.read(cx).value().to_string();
        let args = self.shell.args_input.read(cx).value().to_string();
        let cwd = self.shell.cwd_input.read(cx).value().to_string();
        let spec = match ShellCommandSpec::parse(&program, &args, &cwd) {
            Ok(spec) => spec,
            Err(error) => {
                self.shell.last_status = "配置无效".into();
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        if !program_exists(&spec.program) {
            self.shell.last_status = "程序不存在".into();
            self.status = Some("找不到指定程序；请检查绝对路径或 PATH".into());
            cx.notify();
            return;
        }

        let display = spec.display();
        self.shell.output_input.update(cx, |input, cx| {
            input.set_value(format!("$ {display}\n\n正在运行…"), window, cx)
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let runner_cancel = cancel.clone();
        let operation = cx
            .background_executor()
            .spawn(async move { execute_shell_command(spec, runner_cancel) });
        self.shell.running = true;
        self.shell.cancel = Some(cancel);
        self.shell.last_status = "运行中".into();
        self.status = Some("命令已直接启动；未经过系统 shell".into());
        self.shell.task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = operation.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.shell.running = false;
                this.shell.cancel = None;
                this.shell.task = None;
                this.shell.run_count = this.shell.run_count.saturating_add(1);
                this.shell.last_status = result.status.clone();
                this.shell.output_input.update(cx, |input, cx| {
                    input.set_value(result.transcript, window, cx)
                });
                this.status = Some(format!("Shell：{}", result.status).into());
                cx.notify();
            });
        }));
        cx.notify();
    }
}
