use schemars::Schema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::agent::{Model, RawCall, Seed, Tool, ToolResult, SEED_CALL_ID, SEED_TOOL};

const MAX_TOKENS: u32 = 80000;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Serialize)]
struct SystemBlock {
    #[serde(rename = "type")]
    kind: TextType,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum TextType {
    Text,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
enum Message {
    User { content: Vec<ContentBlock> },
    Assistant { content: Vec<AssistantBlock> },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AssistantBlock {
    Text {
        text: String,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    Thinking {
        thinking: String,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    RedactedThinking {
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
}

#[derive(Serialize)]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Schema,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
enum ToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

pub struct AnthropicAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    system: Vec<SystemBlock>,
    messages: Vec<Message>,
    tools: Vec<ToolDef>,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: &'a [Message],
    tools: &'a [ToolDef],
    tool_choice: ToolChoice,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    system: &'a [SystemBlock],
}

impl AnthropicAgent {
    pub fn new(key: String, model: String, seed: &Seed, tools: &[Tool]) -> Self {
        let system = vec![SystemBlock {
            kind: TextType::Text,
            text: seed.system.to_string(),
        }];
        let messages = vec![
            Message::User {
                content: vec![ContentBlock::Text {
                    text: seed.transform.to_string(),
                }],
            },
            Message::Assistant {
                content: vec![AssistantBlock::ToolUse {
                    id: SEED_CALL_ID.to_string(),
                    name: SEED_TOOL.to_string(),
                    input: Seed::seed_call_args(),
                    extra: Map::new(),
                }],
            },
            Message::User {
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: SEED_CALL_ID.to_string(),
                    content: seed.selected.to_string(),
                    is_error: false,
                }],
            },
        ];
        let tools = tools
            .iter()
            .map(|t| ToolDef {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema: t.input_schema.clone(),
            })
            .collect();
        AnthropicAgent {
            key,
            model,
            client: crate::backend::http_client(),
            system,
            messages,
            tools,
        }
    }

    fn request(&self) -> Request<'_> {
        Request {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            messages: &self.messages,
            tools: &self.tools,
            tool_choice: ToolChoice::Auto,
            system: &self.system,
        }
    }
}

impl Model for AnthropicAgent {
    fn turn(&mut self, results: Vec<ToolResult>) -> anyhow::Result<Vec<RawCall>> {
        if !results.is_empty() {
            let content = results
                .into_iter()
                .map(|r| {
                    let (content, is_error) = match r.result {
                        Ok(c) => (c, false),
                        Err(c) => (c, true),
                    };
                    ContentBlock::ToolResult {
                        tool_use_id: r.id,
                        content,
                        is_error,
                    }
                })
                .collect();
            self.messages.push(Message::User { content });
        }

        let body = post(&self.client, &self.key, &self.request())?;
        let content = body
            .get("content")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Anthropic response missing content: {body}"))?;
        let content: Vec<AssistantBlock> = serde_json::from_value(content)
            .map_err(|e| anyhow::anyhow!("Anthropic content did not parse: {e}"))?;
        let calls = calls_from_content(&content);
        self.messages.push(Message::Assistant { content });
        Ok(calls)
    }
}

fn calls_from_content(content: &[AssistantBlock]) -> Vec<RawCall> {
    content
        .iter()
        .filter_map(|b| match b {
            AssistantBlock::ToolUse {
                id, name, input, ..
            } => Some(RawCall {
                id: id.clone(),
                name: name.clone(),
                args: input.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn post(client: &reqwest::blocking::Client, key: &str, req: &Request) -> anyhow::Result<Value> {
    tracing::debug!(
        "anthropic request: {}",
        serde_json::to_value(req).unwrap_or_default()
    );
    crate::backend::send_json(
        client
            .post(API_URL)
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(req),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request_json(agent: &AnthropicAgent) -> Value {
        serde_json::to_value(agent.request()).unwrap()
    }

    #[test]
    fn tool_choice_serializes_to_wire_shape() {
        assert_eq!(
            serde_json::to_value(ToolChoice::Auto).unwrap(),
            json!({ "type": "auto" })
        );
        assert_eq!(
            serde_json::to_value(ToolChoice::Any).unwrap(),
            json!({ "type": "any" })
        );
        assert_eq!(
            serde_json::to_value(ToolChoice::Tool { name: "edit".into() }).unwrap(),
            json!({ "type": "tool", "name": "edit" })
        );
    }

    #[test]
    fn agent_request_carries_tools_and_seed() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let agent = AnthropicAgent::new("k".into(), "claude-opus-4-8".into(), &seed, &tools);
        let req = request_json(&agent);

        assert_eq!(req["system"][0]["type"], "text");
        assert_eq!(req["system"][0]["text"], "SYS");
        assert_eq!(req["messages"][0]["role"], "user");
        assert_eq!(req["messages"][0]["content"][0]["type"], "text");
        assert_eq!(req["messages"][0]["content"][0]["text"], "transform");
        assert_eq!(req["messages"][0]["content"][1], Value::Null);
        assert_eq!(req["messages"][1]["role"], "assistant");
        assert_eq!(req["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(req["messages"][1]["content"][0]["name"], "view");
        let seed_id = req["messages"][1]["content"][0]["id"].clone();
        assert_eq!(req["messages"][2]["role"], "user");
        assert_eq!(req["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(req["messages"][2]["content"][0]["tool_use_id"], seed_id);
        assert_eq!(req["messages"][2]["content"][0]["content"], "selected");
        assert_eq!(req["tool_choice"]["type"], "auto");
        let names: Vec<&str> = req["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["edit", "view", "reset", "finish"]);
    }

    #[test]
    fn tool_result_turn_serializes_to_wire_shape() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let mut agent = AnthropicAgent::new("k".into(), "m".into(), &seed, &tools);
        agent.messages.push(Message::User {
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: "ok".into(),
                is_error: false,
            }],
        });
        let req = request_json(&agent);
        let block = &req["messages"][3]["content"][0];
        assert_eq!(req["messages"][3]["role"], "user");
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "tu_1");
        assert_eq!(block["content"], "ok");
        assert_eq!(block["is_error"], false);
    }

    #[test]
    fn echoed_assistant_turn_is_verbatim() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let mut agent = AnthropicAgent::new("k".into(), "m".into(), &seed, &tools);
        let raw = json!([
            { "type": "thinking", "thinking": "hmm", "signature": "sig" },
            { "type": "tool_use", "id": "tu_1", "name": "edit", "input": { "old": "a", "new": "b" } }
        ]);
        let content: Vec<AssistantBlock> = serde_json::from_value(raw.clone()).unwrap();
        agent.messages.push(Message::Assistant { content });
        let req = request_json(&agent);
        assert_eq!(req["messages"][3]["role"], "assistant");
        assert_eq!(req["messages"][3]["content"], raw);
    }

    #[test]
    fn parses_tool_use_blocks() {
        let content: Vec<AssistantBlock> = serde_json::from_value(json!([
            { "type": "text", "text": "let me fix that" },
            { "type": "tool_use", "id": "tu_1", "name": "edit",
              "input": { "old": "a", "new": "b" } },
            { "type": "tool_use", "id": "tu_2", "name": "finish", "input": {} }
        ]))
        .unwrap();
        let calls = calls_from_content(&content);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].name, "edit");
        assert_eq!(calls[0].args["old"], "a");
        assert_eq!(calls[1].name, "finish");
    }

    #[test]
    fn no_tool_use_is_no_calls() {
        let content: Vec<AssistantBlock> =
            serde_json::from_value(json!([{ "type": "text", "text": "all done" }])).unwrap();
        assert!(calls_from_content(&content).is_empty());
    }
}
