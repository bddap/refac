//! OpenAI chat-completions API edit-mode agent.

use anyhow::Context;
use serde_json::{json, Value};

use crate::agent::{Model, RawCall, Seed, ToolResult, ToolSpec};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

/// An edit-mode session against the chat-completions API. Implements [`Model`]:
/// each `turn` first threads the previous turn's results back as `role: "tool"`
/// messages, posts the running conversation plus the function tools, and returns
/// the model's `tool_calls`. The assistant message is echoed verbatim so the
/// `tool_call_id`s line up — and every tool call gets a result, which the API
/// requires.
pub struct OpenaiAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    messages: Vec<Value>,
    tools: Vec<Value>,
}

impl OpenaiAgent {
    pub fn new(key: String, model: String, seed: &Seed, tools: &[ToolSpec]) -> Self {
        // Selected and transform stay separate user messages, keeping the
        // boundary explicit. OpenAI accepts empty content, so no placeholder.
        let messages = vec![
            json!({ "role": "system", "content": seed.system }),
            json!({ "role": "user", "content": seed.selected }),
            json!({ "role": "user", "content": seed.transform }),
        ];
        let tools = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
        OpenaiAgent {
            key,
            model,
            client: crate::agent::http_client(),
            messages,
            tools,
        }
    }

    fn request(&self) -> Value {
        json!({
            "model": self.model,
            "messages": self.messages,
            "tools": self.tools,
            "tool_choice": "auto",
        })
    }
}

impl Model for OpenaiAgent {
    fn turn(&mut self, results: Vec<ToolResult>) -> anyhow::Result<Vec<RawCall>> {
        // Answer the previous turn's tool calls first. chat-completions has no
        // error flag on a tool message, so mark failures in the content.
        for r in results {
            let content = if r.is_error {
                format!("ERROR: {}", r.content)
            } else {
                r.content
            };
            self.messages.push(json!({
                "role": "tool",
                "tool_call_id": r.id,
                "content": content,
            }));
        }

        let body = post(&self.client, &self.key, &self.request())?;
        let message = body["choices"][0]["message"].clone();
        if message.is_null() {
            anyhow::bail!("OpenAI response missing a message: {body}");
        }
        self.messages.push(message.clone());
        Ok(calls_from_message(&message))
    }
}

/// Pull `tool_calls` out of an assistant message; each `arguments` is a JSON
/// string to parse.
fn calls_from_message(message: &Value) -> Vec<RawCall> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let function = c.get("function")?;
            let args = function
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| json!({}));
            Some(RawCall {
                id: c.get("id")?.as_str()?.to_string(),
                name: function.get("name")?.as_str()?.to_string(),
                args,
            })
        })
        .collect()
}

fn post(client: &reqwest::blocking::Client, key: &str, req: &Value) -> anyhow::Result<Value> {
    let response = client
        .post(API_URL)
        .bearer_auth(key)
        .json(req)
        .send()
        .context("Failed to send request to OpenAI API")?;
    let status = response.status();
    let body = response
        .json::<Value>()
        .with_context(|| anyhow::anyhow!("Status: {status}. Failed to parse response body."))?;
    if !status.is_success() {
        let pretty = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());
        anyhow::bail!("Status: {status}. Body: {pretty}");
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_request_uses_function_tools() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let agent = OpenaiAgent::new("k".into(), "gpt-5.5".into(), &seed, &tools);
        let req = agent.request();

        assert_eq!(req["tool_choice"], "auto");
        assert_eq!(req["messages"][0]["content"], "SYS");
        assert_eq!(req["messages"][1]["content"], "selected");
        assert_eq!(req["messages"][2]["content"], "transform");
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
    fn parses_tool_calls_with_string_arguments() {
        let message = json!({
            "role": "assistant",
            "tool_calls": [
                { "id": "c1", "type": "function",
                  "function": { "name": "edit", "arguments": "{\"old\":\"a\",\"new\":\"b\"}" } },
                { "id": "c2", "type": "function",
                  "function": { "name": "finish", "arguments": "{}" } }
            ]
        });
        let calls = calls_from_message(&message);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].name, "edit");
        assert_eq!(calls[0].args["new"], "b");
        assert_eq!(calls[1].name, "finish");
    }

    #[test]
    fn no_tool_calls_is_no_calls() {
        let message = json!({ "role": "assistant", "content": "done" });
        assert!(calls_from_message(&message).is_empty());
    }
}
