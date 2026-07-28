//! Desktop external ACP client.
//!
//! A command is accepted only from local user settings, parsed into explicit
//! argv, and spawned directly by the official ACP Rust SDK. No shell is used.
//! This adapter intentionally denies every permission request until ideall has
//! an interactive per-request approval surface.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use agent_client_protocol::schema::{ProtocolVersion, v1::*};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use agent_client_protocol::{on_receive_notification, on_receive_request};
use futures::future::{Either, select};
use thiserror::Error;

const MAX_PROGRAM_CHARS: usize = 512;
const MAX_ARGS_TEXT_CHARS: usize = 8 * 1024;
const MAX_ARGUMENTS: usize = 128;
const MAX_ARGUMENT_CHARS: usize = 4 * 1024;
const MAX_PROMPT_BYTES: usize = 512 * 1024;
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalAcpConfig {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl ExternalAcpConfig {
    pub fn parse(program: &str, args_text: &str, cwd: &str) -> Result<Self, AcpConfigError> {
        let program = program.trim();
        if program.is_empty()
            || program.chars().count() > MAX_PROGRAM_CHARS
            || contains_command_control(program)
        {
            return Err(AcpConfigError::InvalidProgram);
        }
        if args_text.chars().count() > MAX_ARGS_TEXT_CHARS || contains_command_control(args_text) {
            return Err(AcpConfigError::InvalidArguments);
        }
        let args = shell_words::split(args_text).map_err(|_| AcpConfigError::InvalidArguments)?;
        let cwd = if cwd.trim().is_empty() {
            std::env::current_dir().map_err(|_| AcpConfigError::InvalidWorkingDirectory)?
        } else {
            PathBuf::from(cwd.trim())
        };
        Self::new(PathBuf::from(program), args, cwd)
    }

    pub fn new(program: PathBuf, args: Vec<String>, cwd: PathBuf) -> Result<Self, AcpConfigError> {
        let program_text = program.to_str().ok_or(AcpConfigError::InvalidProgram)?;
        if program_text.trim().is_empty()
            || program_text.chars().count() > MAX_PROGRAM_CHARS
            || contains_command_control(program_text)
        {
            return Err(AcpConfigError::InvalidProgram);
        }
        if args.len() > MAX_ARGUMENTS
            || args.iter().any(|argument| {
                argument.chars().count() > MAX_ARGUMENT_CHARS
                    || argument
                        .chars()
                        .any(|character| matches!(character, '\0' | '\r' | '\n'))
            })
        {
            return Err(AcpConfigError::InvalidArguments);
        }
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err(AcpConfigError::InvalidWorkingDirectory);
        }
        Ok(Self { program, args, cwd })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum AcpConfigError {
    #[error("ACP program must contain 1 to 512 characters without command controls")]
    InvalidProgram,
    #[error("ACP arguments are invalid, unbalanced, or exceed the local argv budget")]
    InvalidArguments,
    #[error("ACP working directory must be an existing absolute directory")]
    InvalidWorkingDirectory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcpToolEvent {
    pub id: String,
    pub title: String,
    pub status: AcpToolStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalAcpResult {
    pub content: String,
    pub stop_reason: String,
    pub tools: Vec<AcpToolEvent>,
    pub denied_permissions: usize,
}

#[derive(Debug, Error)]
pub enum AcpRunError {
    #[error(transparent)]
    Config(#[from] AcpConfigError),
    #[error("ACP prompt must contain 1 to 524288 bytes")]
    InvalidPrompt,
    #[error("external ACP Agent timed out after 10 minutes")]
    Timeout,
    #[error("external ACP Agent failed: {0}")]
    Protocol(String),
    #[error("external ACP Agent response exceeded the local output budget")]
    OutputTooLarge,
}

#[derive(Default)]
struct RunState {
    text: String,
    tools: BTreeMap<String, AcpToolEvent>,
    denied_permissions: usize,
    output_too_large: bool,
}

pub fn run_external_acp(
    config: ExternalAcpConfig,
    prompt: &str,
) -> Result<ExternalAcpResult, AcpRunError> {
    let prompt = prompt.trim();
    if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES {
        return Err(AcpRunError::InvalidPrompt);
    }
    futures::executor::block_on(async {
        let operation = run_external_acp_async(config, prompt.to_owned());
        let timeout = async_io::Timer::after(RUN_TIMEOUT);
        futures::pin_mut!(operation, timeout);
        match select(operation, timeout).await {
            Either::Left((result, _)) => result,
            Either::Right((_, _)) => Err(AcpRunError::Timeout),
        }
    })
}

async fn run_external_acp_async(
    config: ExternalAcpConfig,
    prompt: String,
) -> Result<ExternalAcpResult, AcpRunError> {
    let state = std::sync::Arc::new(std::sync::Mutex::new(RunState::default()));
    let notification_state = state.clone();
    let permission_state = state.clone();
    let server = McpServer::Stdio(
        McpServerStdio::new("ideall-external-agent", config.program).args(config.args),
    );
    let agent = AcpAgent::new(server);
    let stop_reason = Client
        .builder()
        .name("ideall")
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                fold_notification(&notification_state, notification);
                Ok(())
            },
            on_receive_notification!(),
        )
        .on_receive_request(
            async move |_request: RequestPermissionRequest, responder, _connection| {
                permission_state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .denied_permissions += 1;
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                ))
            },
            on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(config.cwd))
                .block_task()
                .await?;
            let response = connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            Ok(format!("{:?}", response.stop_reason).to_ascii_lowercase())
        })
        .await
        .map_err(|error| AcpRunError::Protocol(bounded_protocol_error(&error.to_string())))?;

    let state = std::sync::Arc::try_unwrap(state)
        .map_err(|_| AcpRunError::Protocol("ACP state did not settle".into()))?
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.output_too_large {
        return Err(AcpRunError::OutputTooLarge);
    }
    Ok(ExternalAcpResult {
        content: state.text,
        stop_reason,
        tools: state.tools.into_values().collect(),
        denied_permissions: state.denied_permissions,
    })
}

fn fold_notification(state: &std::sync::Mutex<RunState>, notification: SessionNotification) {
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match notification.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text) = chunk.content {
                if state.text.len().saturating_add(text.text.len()) > MAX_OUTPUT_BYTES {
                    state.output_too_large = true;
                } else {
                    state.text.push_str(&text.text);
                }
            }
        }
        SessionUpdate::ToolCall(tool) => {
            let id = tool.tool_call_id.to_string();
            state.tools.insert(
                id.clone(),
                AcpToolEvent {
                    id,
                    title: bounded_tool_title(&tool.title),
                    status: tool_status(tool.status),
                },
            );
        }
        SessionUpdate::ToolCallUpdate(update) => {
            let id = update.tool_call_id.to_string();
            let event = state.tools.entry(id.clone()).or_insert(AcpToolEvent {
                id,
                title: "外部 Agent 工具".into(),
                status: AcpToolStatus::Pending,
            });
            if let Some(title) = update.fields.title {
                event.title = bounded_tool_title(&title);
            }
            if let Some(status) = update.fields.status {
                event.status = tool_status(status);
            }
        }
        _ => {}
    }
}

fn tool_status(status: ToolCallStatus) -> AcpToolStatus {
    match status {
        ToolCallStatus::Pending => AcpToolStatus::Pending,
        ToolCallStatus::InProgress => AcpToolStatus::InProgress,
        ToolCallStatus::Completed => AcpToolStatus::Completed,
        ToolCallStatus::Failed => AcpToolStatus::Failed,
        _ => AcpToolStatus::Pending,
    }
}

fn bounded_tool_title(value: &str) -> String {
    let value = value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect::<String>();
    if value.trim().is_empty() {
        "外部 Agent 工具".into()
    } else {
        value
    }
}

fn bounded_protocol_error(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(240)
        .collect()
}

fn contains_command_control(value: &str) -> bool {
    value
        .chars()
        .any(|character| matches!(character, '\0' | '\r' | '\n'))
}

pub fn command_exists(program: &Path) -> bool {
    if program.components().count() > 1 {
        return program.is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| {
            let candidate = directory.join(program);
            if candidate.is_file() {
                return true;
            }
            #[cfg(windows)]
            {
                ["exe", "cmd", "bat", "com"]
                    .iter()
                    .any(|extension| candidate.with_extension(extension).is_file())
            }
            #[cfg(not(windows))]
            false
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_argv_without_shell_expansion() {
        let cwd = std::env::current_dir().unwrap();
        let config = ExternalAcpConfig::parse(
            "agent",
            r#"--flag "two words" '$HOME' '$(touch nope)'"#,
            cwd.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            config.args,
            vec!["--flag", "two words", "$HOME", "$(touch nope)"]
        );
    }

    #[test]
    fn rejects_controls_unbalanced_quotes_and_relative_working_directories() {
        assert!(matches!(
            ExternalAcpConfig::parse("bad\ncommand", "", ""),
            Err(AcpConfigError::InvalidProgram)
        ));
        assert!(matches!(
            ExternalAcpConfig::parse("agent", "'open", ""),
            Err(AcpConfigError::InvalidArguments)
        ));
        assert!(matches!(
            ExternalAcpConfig::parse("agent", "", "relative"),
            Err(AcpConfigError::InvalidWorkingDirectory)
        ));
    }

    #[test]
    fn folds_text_and_redacted_tool_state_without_raw_payloads() {
        let state = std::sync::Mutex::new(RunState::default());
        fold_notification(
            &state,
            SessionNotification::new(
                "s",
                SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                    TextContent::new("hello"),
                ))),
            ),
        );
        fold_notification(
            &state,
            SessionNotification::new(
                "s",
                SessionUpdate::ToolCall(
                    ToolCall::new("tc", "读取秘密路径").status(ToolCallStatus::Completed),
                ),
            ),
        );
        let state = state.into_inner().unwrap();
        assert_eq!(state.text, "hello");
        assert_eq!(state.tools["tc"].status, AcpToolStatus::Completed);
        assert_eq!(state.tools["tc"].title, "读取秘密路径");
    }
}
