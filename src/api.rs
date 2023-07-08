use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::api_client::{Endpoint, Req};

/// Represents a request for an edit.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EditRequest {
    /// ID of the model to use. You can use the text-davinci-edit-001 or
    /// code-davinci-edit-001 model with this endpoint.
    pub model: String,
    /// The input text to use as a starting point for the edit. Defaults to an
    /// empty string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// The instruction that tells the model how to edit the prompt.
    pub instruction: String,
    /// How many edits to generate for the input and instruction. Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// What sampling temperature to use, between 0 and 2. Higher values like
    /// 0.8 will make the output more random, while lower values like 0.2 will
    /// make it more focused and deterministic. Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// An alternative to sampling with temperature, called nucleus sampling,
    /// where the model considers the results of the tokens with top_p
    /// probability mass. So 0.1 means only the tokens comprising the top 10%
    /// probability mass are considered. Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
}

impl Endpoint for EditRequest {
    type Response = EditResponse;

    fn req(&self) -> Req {
        Req::new(Method::POST, "/v1/edits")
            .header("Content-Type", "application/json")
            .json(self)
    }
}

/// Represents a response from the "edits" endpoint.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EditResponse {
    /// The object type, in this case, "edit".
    pub object: String,
    /// The timestamp when the edit was created.
    pub created: u64,
    /// A vector of the generated edit choices.
    pub choices: Vec<Choice>,
    /// Information about token usage.
    pub usage: Usage,
}

/// Represents an individual edit choice.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Choice {
    /// The edited text.
    pub text: String,
    /// The index of the choice in the response.
    pub index: u32,
}

/// Represents the token usage information in the response.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Usage {
    /// The number of tokens used for the prompt.
    pub prompt_tokens: u32,
    /// The number of tokens used for the completion.
    pub completion_tokens: Option<u32>,
    /// The total number of tokens used.
    pub total_tokens: u32,
}

/// Represents a chat message.
/// serialized examples
/// ```json
/// {"role": "system", "content": "You are a helpful chat bot."}
/// {"role": "user", "content": "What is the weather like in Boston?"},
/// {"role": "assistant", "content": null, "function_call": {"name": "get_current_weather", "arguments": "{ \"location\": \"Boston, MA\"}"}},
/// {"role": "function", "name": "get_current_weather", "content": "{\"temperature\": "22", \"unit\": \"celsius\", \"description\": \"Sunny\"}"}
/// ```
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        content: Option<String>,
        function_call: Option<FunctionCall>,
    },
    /// The result of a function call.
    #[serde(rename = "function")]
    Function { name: String, content: String },
}

impl Message {
    pub fn system<S: Into<String>>(content: S) -> Message {
        Message::System {
            content: content.into(),
        }
    }

    pub fn user<S: Into<String>>(content: S) -> Message {
        Message::User {
            content: content.into(),
        }
    }

    pub fn try_into_assistant_content(self) -> Option<String> {
        match self {
            Self::Assistant {
                content: Some(content),
                function_call: None,
            } => Some(content),
            _ => None,
        }
    }

    pub fn assistant_calls<S: Into<String>, A: Into<String>>(name: S, arguments: A) -> Message {
        Message::Assistant {
            content: None,
            function_call: Some(FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            }),
        }
    }
}

/// Represents a function call requested by the llm.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Represents a request for a chat completion.
///
/// A `ChatCompletionRequest` is used to generate completions for chat conversations
/// with the OpenAI API. It contains various parameters that allow
/// control over the behavior of the model, such as temperature, top_p, and max_tokens.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionRequest {
    /// The ID of the model to use (e.g., "gpt-3.5-turbo").
    pub model: String,
    /// The sequence of chat messages to generate completions for.
    pub messages: Vec<Message>,
    /// The sampling temperature to use, between 0 and 2. Higher values make output more random, lower values make it more focused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// The proportion of probability mass to consider when generating completions. Only tokens comprising the top_p probability mass are considered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// The number of chat completion choices to generate for each input message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Whether to enable streaming mode, receiving partial message deltas and tokens as soon as they're available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Up to 4 sequences where the API will stop generating further tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// The maximum number of tokens to generate in the chat completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// A positive value will penalize new tokens based on whether they appear in the text so far, increasing the model's likelihood to talk about new topics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    /// A positive value will penalize new tokens based on their existing frequency in the text so far, decreasing the model's likelihood to repeat the same line verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    /// A JSON object that maps tokens to an associated bias value from -100 to 100, modifying the likelihood of specified tokens appearing in the completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<HashMap<String, f32>>,
    /// A unique identifier representing your end-user, helping OpenAI monitor and detect abuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Which functions the model has access to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<Vec<FunctionSpec>>,
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
pub struct FunctionSpec {
    pub name: String,
    pub description: String,
    pub params: Vec<schemars::schema::Schema>,
}

/// Represents a response from the "chat/completions" endpoint.
///
/// This struct is returned after sending a ChatCompletionRequest to the OpenAI API.
/// It contains the generated chat completion choices and information about API usage.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionResponse {
    /// The ID of the chat completion.
    pub id: String,
    /// The object type (e.g., "chat.completion").
    pub object: String,
    /// The timestamp when the chat completion was created.
    pub created: u64,
    /// The generated chat completion choices.
    pub choices: Vec<ChatChoice>,
    /// Information about the API usage, including prompt, completion, and total token counts.
    pub usage: Usage,
}

/// Represents an individual chat choice.
///
/// A `ChatChoice` is part of the `ChatCompletionResponse` and contains information about
/// an individual choice generated by the model, such as the generated message and the
/// reason the conversation finished.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatChoice {
    /// The index of the chat choice.
    pub index: u32,
    /// The generated message, including the role ("assistant") and content.
    pub message: Message,
    /// The reason why the conversation finished, e.g., "stop".
    pub finish_reason: String,
}
