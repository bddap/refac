//! OpenAI chat-completions backend and its wire types.

use std::collections::HashMap;

use anyhow::Context;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{Model, RawCall, ToolResult, ToolSpec};
use crate::api::{Message, Role};
use crate::api_client::{Client, Endpoint, Req};
use crate::backend::Backend;

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

/// The OpenAI backend: an API key and the model to call.
pub struct Openai {
    key: String,
    model: String,
}

impl Openai {
    pub fn new(key: String, model: String) -> Self {
        Openai { key, model }
    }
}

impl Backend for Openai {
    fn complete(&self, messages: &[Message]) -> anyhow::Result<String> {
        send(&self.key, &self.model, messages)
    }
}

/// Send refac's messages to the OpenAI chat-completions API and return the text.
fn send(api_key: &str, model: &str, messages: &[Message]) -> anyhow::Result<String> {
    let client = Client::new(api_key);

    // OpenAI takes one string per message; sending each field as its own message
    // keeps a boundary between the selected text and the transform.
    let messages: Vec<OpenAiMessage> = messages
        .iter()
        .flat_map(|m| {
            m.fields.iter().map(move |f| OpenAiMessage {
                role: m.role,
                content: f.clone(),
            })
        })
        .collect();

    let request = ChatCompletionRequest {
        model: model.to_string(),
        messages,
        temperature: None,
        top_p: None,
        n: None,
        stream: None,
        stop: None,
        max_tokens: None,
        presence_penalty: None,
        frequency_penalty: None,
        logit_bias: None,
        user: None,
    };

    let response = client.request(&request)?;

    response
        .choices
        .into_iter()
        .next()
        .ok_or(anyhow::anyhow!("No choices returned."))
        .map(|choice| choice.message.content)
}

/// A message in OpenAI's chat wire format (single `content` string).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct OpenAiMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<HashMap<String, f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl Endpoint for ChatCompletionRequest {
    type Response = ChatCompletionResponse;

    fn req(&self) -> Req {
        Req::new(Method::POST, "/v1/chat/completions")
            .header("Content-Type", "application/json")
            .json(self)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatChoice {
    pub index: u32,
    pub message: OpenAiMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: Option<u32>,
    pub total_tokens: u32,
}

/// An edit-mode session against the chat-completions API. Implements [`Model`]:
/// each `turn` posts the running conversation plus the function tools and returns
/// the model's `tool_calls`; `respond` threads results back as `role: "tool"`
/// messages. The assistant message is echoed verbatim so the `tool_call_id`s line
/// up — and every tool call gets a result, which the API requires.
pub struct OpenaiAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    messages: Vec<Value>,
    tools: Vec<Value>,
}

impl OpenaiAgent {
    pub fn new(key: String, model: String, seed: &[Message], tools: &[ToolSpec]) -> Self {
        // One message per field keeps the selected/transform boundary, as the
        // rewrite path does.
        let mut messages = Vec::new();
        for m in seed {
            for f in &m.fields {
                messages.push(json!({ "role": m.role.as_str(), "content": f }));
            }
        }
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
        let seed = vec![
            Message::system("SYS"),
            Message::user(vec!["selected".into(), "transform".into()]),
        ];
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
