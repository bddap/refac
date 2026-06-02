//! OpenAI chat-completions backend and its wire types.

use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::api::{Message, Role};
use crate::api_client::{Client, Endpoint, Req};

/// Send refac's messages to the OpenAI chat-completions API and return the text.
pub fn complete(api_key: &str, model: &str, messages: &[Message]) -> anyhow::Result<String> {
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
