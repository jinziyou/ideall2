//! Bounded OpenAI-compatible model transport used by the native Agent.
//!
//! The adapter deliberately implements the small common denominator needed by
//! ideall: non-streaming `chat/completions` plus function tools. Provider
//! credentials are supplied by the caller for one process-local client and are
//! never serialized into settings or error messages.

use std::{collections::HashSet, io::Read as _, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::Url;

const API_KEY_MAX_BYTES: usize = 16 * 1024;
const MODEL_MAX_CHARS: usize = 256;
const MAX_MESSAGES: usize = 256;
const MAX_TOOLS: usize = 64;
const MAX_TOOL_CALLS_PER_MESSAGE: usize = 16;
const REQUEST_LIMIT: usize = 2 * 1024 * 1024;
const RESPONSE_LIMIT: u64 = 4 * 1024 * 1024;
const MESSAGE_TEXT_LIMIT: usize = 1024 * 1024;
const TOOL_ARGUMENT_LIMIT: usize = 512 * 1024;
const ERROR_TEXT_LIMIT: usize = 240;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ModelToolFunction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: ModelRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ModelToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ModelMessage {
    pub fn text(role: ModelRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Tool,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub trait CompletionProvider {
    fn complete(
        &mut self,
        messages: &[ModelMessage],
        tools: &[ModelTool],
    ) -> Result<ModelMessage, OpenAiError>;
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum OpenAiConfigError {
    #[error("model endpoint must be an http(s) base URL without credentials, query, or fragment")]
    InvalidBaseUrl,
    #[error("model name must contain 1 to 256 printable characters")]
    InvalidModel,
    #[error("model API key is empty, oversized, or contains invalid header characters")]
    InvalidApiKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum OpenAiError {
    #[error("model request is invalid: {0}")]
    InvalidRequest(String),
    #[error("model request exceeds the local privacy budget")]
    RequestTooLarge,
    #[error("model response exceeds the local response budget")]
    ResponseTooLarge,
    #[error("model network request failed")]
    Network,
    #[error("model endpoint returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("model endpoint returned an invalid response")]
    InvalidResponse,
}

pub struct OpenAiCompatibleClient {
    agent: ureq::Agent,
    endpoint: Url,
    model: String,
    authorization: String,
}

impl OpenAiCompatibleClient {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, OpenAiConfigError> {
        let mut endpoint = canonical_base_url(base_url)?;
        endpoint
            .path_segments_mut()
            .map_err(|_| OpenAiConfigError::InvalidBaseUrl)?
            .pop_if_empty()
            .extend(["chat", "completions"]);
        let model = validate_model_name(model)?;
        if api_key.is_empty()
            || api_key.len() > API_KEY_MAX_BYTES
            || !api_key.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(OpenAiConfigError::InvalidApiKey);
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(90)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.into(),
            endpoint,
            model,
            authorization: format!("Bearer {api_key}"),
        })
    }
}

impl CompletionProvider for OpenAiCompatibleClient {
    fn complete(
        &mut self,
        messages: &[ModelMessage],
        tools: &[ModelTool],
    ) -> Result<ModelMessage, OpenAiError> {
        validate_request(messages, tools)?;
        let body = serde_json::to_vec(&CompletionRequest {
            model: &self.model,
            messages,
            tools: tools
                .iter()
                .map(|tool| ToolEnvelope {
                    kind: "function",
                    function: ToolDefinition {
                        name: &tool.name,
                        description: &tool.description,
                        parameters: &tool.input_schema,
                    },
                })
                .collect(),
            stream: false,
        })
        .map_err(|_| OpenAiError::InvalidRequest("messages cannot be serialized".into()))?;
        if body.len() > REQUEST_LIMIT {
            return Err(OpenAiError::RequestTooLarge);
        }
        let mut response = self
            .agent
            .post(self.endpoint.as_str())
            .header("Authorization", &self.authorization)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send(body.as_slice())
            .map_err(|_| OpenAiError::Network)?;
        let status = response.status().as_u16();
        let body = read_bounded_body(&mut response)?;
        if !(200..300).contains(&status) {
            return Err(OpenAiError::Http {
                status,
                message: provider_error_message(&body),
            });
        }
        let response: CompletionResponse =
            serde_json::from_slice(&body).map_err(|_| OpenAiError::InvalidResponse)?;
        let message = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or(OpenAiError::InvalidResponse)?;
        validate_assistant_message(&message)?;
        Ok(message)
    }
}

pub fn canonical_base_url(value: &str) -> Result<Url, OpenAiConfigError> {
    let mut url = Url::parse(value.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .filter(|url| url.host_str().is_some())
        .filter(|url| url.username().is_empty() && url.password().is_none())
        .filter(|url| url.query().is_none() && url.fragment().is_none())
        .ok_or(OpenAiConfigError::InvalidBaseUrl)?;
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

pub fn validate_model_name(value: &str) -> Result<String, OpenAiConfigError> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MODEL_MAX_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(OpenAiConfigError::InvalidModel);
    }
    Ok(value.to_owned())
}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: &'a [ModelMessage],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolEnvelope<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct ToolEnvelope<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: ToolDefinition<'a>,
}

#[derive(Serialize)]
struct ToolDefinition<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a Value,
}

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    message: ModelMessage,
}

fn validate_request(messages: &[ModelMessage], tools: &[ModelTool]) -> Result<(), OpenAiError> {
    if messages.is_empty() || messages.len() > MAX_MESSAGES {
        return Err(OpenAiError::InvalidRequest(format!(
            "message count must be between 1 and {MAX_MESSAGES}"
        )));
    }
    for message in messages {
        validate_message(message)?;
    }
    if tools.len() > MAX_TOOLS {
        return Err(OpenAiError::InvalidRequest(format!(
            "tool count exceeds {MAX_TOOLS}"
        )));
    }
    let mut names = HashSet::with_capacity(tools.len());
    for tool in tools {
        if !valid_name(&tool.name)
            || !names.insert(tool.name.as_str())
            || !tool.input_schema.is_object()
            || tool.description.chars().count() > 2_048
            || tool.description.chars().any(char::is_control)
        {
            return Err(OpenAiError::InvalidRequest(
                "tool definitions are invalid".into(),
            ));
        }
    }
    Ok(())
}

fn validate_message(message: &ModelMessage) -> Result<(), OpenAiError> {
    if message
        .content
        .as_ref()
        .is_some_and(|content| content.len() > MESSAGE_TEXT_LIMIT)
    {
        return Err(OpenAiError::InvalidRequest(
            "message content is oversized".into(),
        ));
    }
    match message.role {
        ModelRole::Tool if message.tool_call_id.as_deref().is_none_or(str::is_empty) => {
            return Err(OpenAiError::InvalidRequest(
                "tool message is missing its call id".into(),
            ));
        }
        ModelRole::Assistant => {
            for call in &message.tool_calls {
                validate_tool_call(call)?;
            }
        }
        ModelRole::System | ModelRole::User | ModelRole::Tool => {
            if !message.tool_calls.is_empty() {
                return Err(OpenAiError::InvalidRequest(
                    "only assistant messages may contain tool calls".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_assistant_message(message: &ModelMessage) -> Result<(), OpenAiError> {
    if message.role != ModelRole::Assistant {
        return Err(OpenAiError::InvalidResponse);
    }
    validate_message(message).map_err(|_| OpenAiError::InvalidResponse)?;
    if message.content.is_none() && message.tool_calls.is_empty() {
        return Err(OpenAiError::InvalidResponse);
    }
    if message.tool_calls.len() > MAX_TOOL_CALLS_PER_MESSAGE {
        return Err(OpenAiError::InvalidResponse);
    }
    let mut ids = HashSet::with_capacity(message.tool_calls.len());
    if message
        .tool_calls
        .iter()
        .any(|call| !ids.insert(call.id.as_str()))
    {
        return Err(OpenAiError::InvalidResponse);
    }
    Ok(())
}

fn validate_tool_call(call: &ModelToolCall) -> Result<(), OpenAiError> {
    if call.kind != "function"
        || call.id.is_empty()
        || call.id.chars().count() > 256
        || call.id.chars().any(char::is_control)
        || !valid_name(&call.function.name)
        || call.function.arguments.len() > TOOL_ARGUMENT_LIMIT
    {
        return Err(OpenAiError::InvalidRequest("tool call is invalid".into()));
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn read_bounded_body(
    response: &mut ureq::http::Response<ureq::Body>,
) -> Result<Vec<u8>, OpenAiError> {
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|_| OpenAiError::Network)?;
    if body.len() as u64 > RESPONSE_LIMIT {
        return Err(OpenAiError::ResponseTooLarge);
    }
    Ok(body)
}

fn provider_error_message(body: &[u8]) -> String {
    let value = serde_json::from_slice::<Value>(body).ok();
    let message = value
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("message").and_then(Value::as_str))
        })
        .unwrap_or("request rejected");
    let normalized = message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(ERROR_TEXT_LIMIT)
        .collect::<String>();
    if normalized.trim().is_empty() {
        "request rejected".into()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead as _, BufReader, Read as _, Write as _},
        net::{TcpListener, TcpStream},
        thread,
    };

    use serde_json::json;

    use super::*;

    fn serve_once(status: &str, response_body: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            )
            .unwrap();
            request
        });
        (format!("http://{address}/v1"), handle)
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut headers = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            headers.push_str(&line);
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        format!("{headers}{}", String::from_utf8(body).unwrap())
    }

    #[test]
    fn completion_is_authenticated_bounded_and_sends_function_tools() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {"name": "fs.list", "arguments": "{}"}
                    }]
                }
            }]
        })
        .to_string();
        let (base, server) = serve_once("200 OK", response);
        let mut client = OpenAiCompatibleClient::new(&base, "test-model", "secret-key").unwrap();
        let output = client
            .complete(
                &[ModelMessage::text(ModelRole::User, "列出文件")],
                &[ModelTool {
                    name: "fs.list".into(),
                    description: "List metadata".into(),
                    input_schema: json!({"type": "object"}),
                }],
            )
            .unwrap();
        assert_eq!(output.tool_calls[0].function.name, "fs.list");
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request.contains("authorization: Bearer secret-key\r\n"));
        let body = request.split("\r\n\r\n").nth(1).unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["tools"][0]["function"]["name"], "fs.list");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn provider_error_is_bounded_without_reflecting_credentials() {
        let message = format!("{}\n", "x".repeat(400));
        let (base, server) = serve_once(
            "401 Unauthorized",
            json!({"error": {"message": message}}).to_string(),
        );
        let mut client = OpenAiCompatibleClient::new(&base, "m", "do-not-reflect").unwrap();
        let error = client
            .complete(&[ModelMessage::text(ModelRole::User, "hello")], &[])
            .unwrap_err();
        let rendered = error.to_string();
        assert!(rendered.contains("HTTP 401"));
        assert!(!rendered.contains("do-not-reflect"));
        assert!(rendered.chars().count() < 300);
        server.join().unwrap();
    }

    #[test]
    fn rejects_credential_urls_bad_keys_and_oversized_context() {
        assert!(matches!(
            OpenAiCompatibleClient::new("https://user@example.com/v1", "m", "key"),
            Err(OpenAiConfigError::InvalidBaseUrl)
        ));
        assert!(matches!(
            OpenAiCompatibleClient::new("https://example.com/v1", "m", "bad\r\nkey"),
            Err(OpenAiConfigError::InvalidApiKey)
        ));
        let messages = vec![ModelMessage::text(ModelRole::User, "x"); MAX_MESSAGES + 1];
        assert!(matches!(
            validate_request(&messages, &[]),
            Err(OpenAiError::InvalidRequest(_))
        ));
    }

    #[test]
    fn invalid_or_duplicate_tool_calls_are_rejected() {
        let call = ModelToolCall {
            id: "same".into(),
            kind: "function".into(),
            function: ModelToolFunction {
                name: "fs.list".into(),
                arguments: "{}".into(),
            },
        };
        let message = ModelMessage {
            role: ModelRole::Assistant,
            content: None,
            tool_calls: vec![call.clone(), call],
            tool_call_id: None,
        };
        assert_eq!(
            validate_assistant_message(&message),
            Err(OpenAiError::InvalidResponse)
        );
    }
}
