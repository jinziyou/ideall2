use std::collections::HashSet;

use serde_json::{Value, json};

use crate::{Grant, Intent};

#[derive(Clone, Debug, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub intent: Intent,
}

pub trait ToolHandler {
    fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String>;
}

pub struct LocalMcpServer<H> {
    grant: Grant,
    consumer_id: String,
    origin: String,
    tools: Vec<McpTool>,
    handler: H,
}

impl<H: ToolHandler> LocalMcpServer<H> {
    pub fn new(
        grant: Grant,
        consumer_id: impl Into<String>,
        origin: impl Into<String>,
        tools: Vec<McpTool>,
        handler: H,
    ) -> Result<Self, String> {
        let mut names = HashSet::with_capacity(tools.len());
        for tool in &tools {
            if tool.name.trim().is_empty() || !names.insert(tool.name.as_str()) {
                return Err("MCP tool names must be non-empty and unique".into());
            }
            if !tool.input_schema.is_object() {
                return Err(format!(
                    "MCP tool `{}` has an invalid input schema",
                    tool.name
                ));
            }
        }
        Ok(Self {
            grant,
            consumer_id: consumer_id.into(),
            origin: origin.into(),
            tools,
            handler,
        })
    }

    pub fn handle(&mut self, request: Value, now: i64) -> Value {
        let Some(request) = request.as_object() else {
            return error(Value::Null, -32600, "Invalid Request");
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return error(id, -32600, "Invalid Request");
        }
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return error(id, -32600, "Invalid Request");
        };
        match method {
            "initialize" => success(
                id,
                json!({
                    "protocolVersion": request
                        .get("params")
                        .and_then(|params| params.get("protocolVersion"))
                        .and_then(Value::as_str)
                        .unwrap_or("2025-06-18"),
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "ideall-native", "version": env!("CARGO_PKG_VERSION")}
                }),
            ),
            "notifications/initialized" => Value::Null,
            "tools/list" => {
                let tools = self
                    .tools
                    .iter()
                    .filter(|tool| self.authorized(tool.intent, now))
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "inputSchema": tool.input_schema
                        })
                    })
                    .collect::<Vec<_>>();
                success(id, json!({"tools": tools}))
            }
            "tools/call" => self.call_tool(id, request.get("params"), now),
            _ => error(id, -32601, "Method not found"),
        }
    }

    fn call_tool(&mut self, id: Value, params: Option<&Value>, now: i64) -> Value {
        let Some(params) = params.and_then(Value::as_object) else {
            return error(id, -32602, "Invalid params");
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error(id, -32602, "Invalid params");
        };
        let Some(tool) = self.tools.iter().find(|tool| tool.name == name) else {
            return error(id, -32601, "Tool not found");
        };
        if !self.authorized(tool.intent, now) {
            // An unauthorized capability is indistinguishable from an absent
            // capability, matching tools/list and preventing permission probes.
            return error(id, -32601, "Tool not found");
        }
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return error(id, -32602, "Invalid params");
        }
        match self.handler.call(name, arguments) {
            Ok(structured) => success(
                id,
                json!({
                    "content": [{"type": "text", "text": compact_summary(&structured)}],
                    "structuredContent": structured,
                    "isError": false
                }),
            ),
            Err(message) => success(
                id,
                json!({
                    "content": [{"type": "text", "text": bounded_error(&message)}],
                    "isError": true
                }),
            ),
        }
    }

    fn authorized(&self, intent: Intent, now: i64) -> bool {
        self.grant
            .authorize(&self.consumer_id, &self.origin, intent, now)
            .is_ok()
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn compact_summary(value: &Value) -> String {
    match value {
        Value::String(value) => value.chars().take(240).collect(),
        Value::Null => "完成".into(),
        Value::Array(values) => format!("返回 {} 项", values.len()),
        Value::Object(values) => format!("返回 {} 个字段", values.len()),
        value => value.to_string(),
    }
}

fn bounded_error(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    normalized.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ideall_protocol::NodeKind;

    use super::*;
    use crate::{Permission, agent_grant};

    #[derive(Default)]
    struct Handler {
        calls: Vec<String>,
    }

    impl ToolHandler for Handler {
        fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
            self.calls.push(name.into());
            Ok(json!({"name": name, "arguments": arguments}))
        }
    }

    fn tool(name: &str, intent: Intent) -> McpTool {
        McpTool {
            name: name.into(),
            description: name.into(),
            input_schema: json!({"type": "object"}),
            intent,
        }
    }

    #[test]
    fn tools_list_and_call_share_the_same_execution_time_grant_gate() {
        let grant = agent_grant(1, None);
        let mut server = LocalMcpServer::new(
            grant,
            "ideall-agent",
            "loopback",
            vec![
                tool("fs.list", Intent::ListFiles),
                tool("fs.read-note", Intent::ReadContent(NodeKind::Note)),
                tool("fs.write-note", Intent::Write(NodeKind::Note)),
            ],
            Handler::default(),
        )
        .unwrap();
        let listed = server.handle(
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            2,
        );
        let names = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["fs.list", "fs.write-note"]);

        let denied = server.handle(
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "fs.read-note", "arguments": {"id": "n1"}}
            }),
            2,
        );
        assert_eq!(denied["error"]["code"], -32601);
    }

    #[test]
    fn explicit_note_read_permission_exposes_only_that_capability() {
        let requested = [
            Permission::FilesRead,
            Permission::NotesRead,
            Permission::IdentityPublish,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let mut grant = agent_grant(1, Some(&requested));
        // agent_grant can only select the configurable subset; NotesRead and
        // IdentityPublish cannot be silently added by workspace JSON.
        assert!(!grant.permissions.contains(&Permission::NotesRead));
        grant.permissions.insert(Permission::NotesRead);
        let mut server = LocalMcpServer::new(
            grant,
            "ideall-agent",
            "loopback",
            vec![tool("fs.read-note", Intent::ReadContent(NodeKind::Note))],
            Handler::default(),
        )
        .unwrap();
        let called = server.handle(
            json!({
                "jsonrpc": "2.0", "id": "call", "method": "tools/call",
                "params": {"name": "fs.read-note", "arguments": {"id": "n1"}}
            }),
            2,
        );
        assert_eq!(called["result"]["isError"], false);
    }

    #[test]
    fn expired_grant_cannot_call_a_tool_that_was_previously_listed() {
        let mut grant = agent_grant(1, None);
        grant.expiry = Some(3);
        let mut server = LocalMcpServer::new(
            grant,
            "ideall-agent",
            "loopback",
            vec![tool("fs.list", Intent::ListFiles)],
            Handler::default(),
        )
        .unwrap();
        assert_eq!(
            server.handle(
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
                2
            )["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            server.handle(
                json!({
                    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": {"name": "fs.list", "arguments": {}}
                }),
                3
            )["error"]["code"],
            -32601
        );
    }
}
