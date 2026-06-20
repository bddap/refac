use schemars::Schema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::agent::{Model, RawCall, Seed, Tool, ToolResult, SEED_CALL_ID, SEED_TOOL};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
enum Message {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
    Assistant(AssistantTurn),
}

#[derive(Serialize, Deserialize)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: FunctionType,
    function: FunctionCall,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct AssistantTurn {
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    role: Option<String>,
    content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize)]
struct ToolDef {
    #[serde(rename = "type")]
    kind: FunctionType,
    function: FunctionDef,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FunctionType {
    Function,
}

#[derive(Serialize)]
struct FunctionDef {
    name: String,
    description: String,
    parameters: Schema,
}

pub struct OpenaiAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    messages: Vec<Message>,
    tools: Vec<ToolDef>,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    messages: &'a [Message],
    tools: &'a [ToolDef],
    tool_choice: &'static str,
}

impl OpenaiAgent {
    pub fn new(key: String, model: String, seed: &Seed, tools: &[Tool]) -> Self {
        let messages = vec![
            Message::System {
                content: seed.system.to_string(),
            },
            Message::User {
                content: seed.transform.to_string(),
            },
            Message::Assistant(AssistantTurn {
                role: None,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: SEED_CALL_ID.to_string(),
                    kind: FunctionType::Function,
                    function: FunctionCall {
                        name: SEED_TOOL.to_string(),
                        arguments: Seed::seed_call_args().to_string(),
                        extra: Map::new(),
                    },
                    extra: Map::new(),
                }]),
                extra: Map::new(),
            }),
            Message::Tool {
                tool_call_id: SEED_CALL_ID.to_string(),
                content: seed.selected.to_string(),
            },
        ];
        let tools = tools
            .iter()
            .map(|t| ToolDef {
                kind: FunctionType::Function,
                function: FunctionDef {
                    name: t.name.to_string(),
                    description: t.description.to_string(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();
        OpenaiAgent {
            key,
            model,
            client: crate::backend::http_client(),
            messages,
            tools,
        }
    }

    fn request(&self) -> Request<'_> {
        Request {
            model: &self.model,
            messages: &self.messages,
            tools: &self.tools,
            tool_choice: "auto",
        }
    }
}

impl Model for OpenaiAgent {
    fn turn(&mut self, results: Vec<ToolResult>) -> anyhow::Result<Vec<RawCall>> {
        for r in results {
            let content = match r.result {
                Ok(c) => c,
                Err(c) => format!("ERROR: {c}"),
            };
            self.messages.push(Message::Tool {
                tool_call_id: r.id,
                content,
            });
        }

        let body = post(&self.client, &self.key, &self.request())?;
        let message = body["choices"][0]["message"].clone();
        if message.is_null() {
            anyhow::bail!("OpenAI response missing a message: {body}");
        }
        let turn: AssistantTurn = serde_json::from_value(message)
            .map_err(|e| anyhow::anyhow!("OpenAI assistant message did not parse: {e}"))?;
        let calls = raw_calls(turn.tool_calls.as_deref().unwrap_or(&[]));
        self.messages.push(Message::Assistant(turn));
        Ok(calls)
    }
}

fn raw_calls(tool_calls: &[ToolCall]) -> Vec<RawCall> {
    tool_calls
        .iter()
        .map(|c| RawCall {
            id: c.id.clone(),
            name: c.function.name.clone(),
            args: serde_json::from_str(&c.function.arguments)
                .unwrap_or_else(|_| serde_json::json!({})),
        })
        .collect()
}

fn post(client: &reqwest::blocking::Client, key: &str, req: &Request) -> anyhow::Result<Value> {
    crate::backend::send_json(client.post(API_URL).bearer_auth(key).json(req))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request_json(agent: &OpenaiAgent) -> Value {
        serde_json::to_value(agent.request()).unwrap()
    }

    #[test]
    fn agent_request_uses_function_tools() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let agent = OpenaiAgent::new("k".into(), "gpt-5.5".into(), &seed, &tools);
        let req = request_json(&agent);

        assert_eq!(req["tool_choice"], "auto");
        assert_eq!(req["messages"][0]["role"], "system");
        assert_eq!(req["messages"][0]["content"], "SYS");
        assert_eq!(req["messages"][1]["role"], "user");
        assert_eq!(req["messages"][1]["content"], "transform");
        assert_eq!(req["messages"][2]["role"], "assistant");
        assert_eq!(req["messages"][2]["tool_calls"][0]["function"]["name"], "view");
        let seed_id = req["messages"][2]["tool_calls"][0]["id"].clone();
        assert_eq!(req["messages"][3]["role"], "tool");
        assert_eq!(req["messages"][3]["tool_call_id"], seed_id);
        assert_eq!(req["messages"][3]["content"], "selected");
        assert_eq!(req["tools"][0]["type"], "function");
        let names: Vec<&str> = req["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
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
        let mut agent = OpenaiAgent::new("k".into(), "m".into(), &seed, &tools);
        agent.messages.push(Message::Tool {
            tool_call_id: "c1".into(),
            content: "ok".into(),
        });
        let req = request_json(&agent);
        let msg = &req["messages"][4];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "c1");
        assert_eq!(msg["content"], "ok");
    }

    #[test]
    fn assistant_turn_serializes_to_wire_shape() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let mut agent = OpenaiAgent::new("k".into(), "m".into(), &seed, &tools);
        let raw = json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [
                { "id": "c1", "type": "function",
                  "function": { "name": "edit", "arguments": "{\"old\":\"a\",\"new\":\"b\"}" } }
            ]
        });
        let turn: AssistantTurn = serde_json::from_value(raw.clone()).unwrap();
        agent.messages.push(Message::Assistant(turn));
        assert_eq!(request_json(&agent)["messages"][4], raw);
        let wire = serde_json::to_string(&agent.request()).unwrap();
        assert_eq!(wire.matches("\"role\":\"assistant\"").count(), 2);
    }

    #[test]
    fn echoed_assistant_turn_retains_unmodeled_fields_without_duplicate_role() {
        let api_msg = json!({
            "role": "assistant",
            "content": null,
            "refusal": null,
            "reasoning": "let me think",
            "tool_calls": [
                { "id": "c1", "type": "function", "index": 0,
                  "function": { "name": "edit", "arguments": "{\"old\":\"a\",\"new\":\"b\"}" } }
            ]
        });
        let turn: AssistantTurn = serde_json::from_value(api_msg.clone()).unwrap();
        let wire = serde_json::to_string(&Message::Assistant(turn)).unwrap();
        assert_eq!(wire.matches("\"role\":\"assistant\"").count(), 1);
        let back: Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(back["refusal"], api_msg["refusal"]);
        assert_eq!(back["reasoning"], api_msg["reasoning"]);
        assert_eq!(back["tool_calls"][0]["index"], api_msg["tool_calls"][0]["index"]);
        assert_eq!(
            back["tool_calls"][0]["function"]["arguments"],
            api_msg["tool_calls"][0]["function"]["arguments"]
        );
    }

    #[test]
    fn assistant_arguments_string_is_byte_identical() {
        let args = "{\"b\": 1, \"a\": 1.0, \"n\": 1e3}";
        let raw = json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [
                { "id": "c1", "type": "function",
                  "function": { "name": "edit", "arguments": args } }
            ]
        });
        let turn: AssistantTurn = serde_json::from_value(raw).unwrap();
        let msg = Message::Assistant(turn);
        assert_eq!(
            serde_json::to_value(&msg).unwrap()["tool_calls"][0]["function"]["arguments"],
            json!(args)
        );
    }

    #[test]
    fn text_only_assistant_turn_omits_tool_calls() {
        let raw = json!({ "role": "assistant", "content": "done" });
        let turn: AssistantTurn = serde_json::from_value(raw).unwrap();
        let msg = Message::Assistant(turn);
        let wire = serde_json::to_value(&msg).unwrap();
        assert_eq!(wire["content"], "done");
        assert!(wire.get("tool_calls").is_none());
    }

    #[test]
    fn parses_tool_calls_with_string_arguments() {
        let raw = json!({
            "role": "assistant",
            "tool_calls": [
                { "id": "c1", "type": "function",
                  "function": { "name": "edit", "arguments": "{\"old\":\"a\",\"new\":\"b\"}" } },
                { "id": "c2", "type": "function",
                  "function": { "name": "finish", "arguments": "{}" } }
            ]
        });
        let turn: AssistantTurn = serde_json::from_value(raw).unwrap();
        let calls = raw_calls(turn.tool_calls.as_deref().unwrap_or(&[]));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].name, "edit");
        assert_eq!(calls[0].args["new"], "b");
        assert_eq!(calls[1].name, "finish");
    }

    #[test]
    fn no_tool_calls_is_no_calls() {
        let raw = json!({ "role": "assistant", "content": "done" });
        let turn: AssistantTurn = serde_json::from_value(raw).unwrap();
        assert!(raw_calls(turn.tool_calls.as_deref().unwrap_or(&[])).is_empty());
    }
}
