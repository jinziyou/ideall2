use std::{
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use gpui::SharedString;

const MAX_ARGUMENTS: usize = 128;
const MAX_ARGUMENT_CHARS: usize = 4 * 1024;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;
const RUN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ShellCommandSpec {
    pub(super) program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

impl ShellCommandSpec {
    pub(super) fn parse(program: &str, args: &str, cwd: &str) -> Result<Self, String> {
        let program = program.trim();
        if program.is_empty() || program.chars().count() > 512 || contains_command_control(program)
        {
            return Err("程序必须包含 1–512 个字符，且不能包含换行或 NUL".into());
        }
        let program = PathBuf::from(program);
        if program.components().count() > 1 && !program.is_absolute() {
            return Err("带目录的程序路径必须是绝对路径".into());
        }
        let args = shell_words::split(args).map_err(|_| "参数引号不完整".to_owned())?;
        if args.len() > MAX_ARGUMENTS
            || args.iter().any(|argument| {
                argument.chars().count() > MAX_ARGUMENT_CHARS || contains_command_control(argument)
            })
        {
            return Err("参数超过 128 项，或单项超过 4096 个字符".into());
        }
        let cwd = PathBuf::from(cwd.trim());
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err("工作目录必须是现有的绝对目录".into());
        }
        Ok(Self { program, args, cwd })
    }

    pub(super) fn display(&self) -> String {
        std::iter::once(self.program.to_string_lossy().to_string())
            .chain(self.args.iter().map(|argument| format!("{argument:?}")))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug)]
pub(super) struct ShellCommandResult {
    pub(super) transcript: String,
    pub(super) status: SharedString,
}

pub(super) fn execute_shell_command(
    spec: ShellCommandSpec,
    cancel: Arc<AtomicBool>,
) -> ShellCommandResult {
    let display = spec.display();
    let started = Instant::now();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    copy_safe_environment(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ShellCommandResult {
                transcript: format!("$ {display}\n\n启动失败：{error}"),
                status: "启动失败".into(),
            };
        }
    };
    let stdout = child.stdout.take().expect("piped stdout is present");
    let stderr = child.stderr.take().expect("piped stderr is present");
    let stdout_reader = thread::spawn(move || read_bounded(stdout));
    let stderr_reader = thread::spawn(move || read_bounded(stderr));

    let (exit_status, canceled, timed_out) = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            break (child.wait().ok(), true, false);
        }
        if started.elapsed() >= RUN_TIMEOUT {
            let _ = child.kill();
            break (child.wait().ok(), false, true);
        }
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false, false),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => break (child.wait().ok(), false, false),
        }
    };

    let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
    let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
    let mut transcript = format!("$ {display}\n");
    if !stdout.is_empty() {
        transcript.push_str("\nstdout\n");
        transcript.push_str(&stdout);
    }
    if !stderr.is_empty() {
        transcript.push_str("\nstderr\n");
        transcript.push_str(&stderr);
    }
    if stdout_truncated || stderr_truncated {
        transcript.push_str("\n\n[输出超过 1 MiB 展示预算，已截断]");
    }
    let elapsed_ms = started.elapsed().as_millis();
    let status = if canceled {
        transcript.push_str(&format!("\n\n[已取消 · {elapsed_ms} ms]"));
        "已取消".into()
    } else if timed_out {
        transcript.push_str("\n\n[运行超过 30 秒，已终止]");
        "已超时".into()
    } else if let Some(exit_status) = exit_status {
        transcript.push_str(&format!(
            "\n\n[退出码 {} · {elapsed_ms} ms]",
            exit_status
                .code()
                .map_or_else(|| "signal".into(), |code| code.to_string())
        ));
        if exit_status.success() {
            "执行成功".into()
        } else {
            "执行失败".into()
        }
    } else {
        transcript.push_str("\n\n[无法读取进程退出状态]");
        "状态未知".into()
    };
    ShellCommandResult { transcript, status }
}

fn read_bounded(reader: impl Read) -> (Vec<u8>, bool) {
    let mut reader = reader;
    let mut output = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 8 * 1024];
    while let Ok(read) = reader.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        truncated |= read > remaining;
    }
    (output, truncated)
}

fn copy_safe_environment(command: &mut Command) {
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "LANG",
        "LC_ALL",
        "TERM",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "COMSPEC",
        "TMP",
        "TEMP",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(OsStr::new(key), value);
        }
    }
}

fn contains_command_control(value: &str) -> bool {
    value
        .chars()
        .any(|character| matches!(character, '\0' | '\r' | '\n'))
}

pub(super) fn program_exists(program: &Path) -> bool {
    ideall_acp::command_exists(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parser_preserves_shell_syntax_as_literal_argv() {
        let cwd = std::env::current_dir().unwrap();
        let spec = ShellCommandSpec::parse(
            "printf",
            r#""two words" '$HOME' '$(touch never)'"#,
            cwd.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(spec.args, vec!["two words", "$HOME", "$(touch never)"]);
    }

    #[test]
    fn command_parser_rejects_relative_program_paths_and_bad_working_directories() {
        let cwd = std::env::current_dir().unwrap();
        assert!(ShellCommandSpec::parse("./script", "", cwd.to_str().unwrap()).is_err());
        assert!(ShellCommandSpec::parse("echo", "", "relative").is_err());
    }

    #[test]
    fn bounded_reader_keeps_draining_but_caps_retained_output() {
        let input = vec![b'x'; MAX_OUTPUT_BYTES + 32];
        let (output, truncated) = read_bounded(input.as_slice());

        assert_eq!(output.len(), MAX_OUTPUT_BYTES);
        assert!(truncated);
    }

    #[test]
    fn command_runner_executes_explicit_argv_without_a_shell() {
        let spec = ShellCommandSpec {
            program: PathBuf::from("rustc"),
            args: vec!["--version".into()],
            cwd: std::env::current_dir().unwrap(),
        };
        let result = execute_shell_command(spec, Arc::new(AtomicBool::new(false)));

        assert_eq!(result.status.as_ref(), "执行成功");
        assert!(result.transcript.contains("rustc "));
    }
}
