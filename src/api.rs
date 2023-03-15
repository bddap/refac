use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::api_client::{Endpoint, Req};

/// Represents a request for an edit.
#[derive(Debug, Serialize, Deserialize)]
pub struct EditRequest {
    /// ID of the model to use. You can use the text-davinci-edit-001 or
    /// code-davinci-edit-001 model with this endpoint.
    pub model: String,
    /// The input text to use as a starting point for the edit. Defaults to an
    /// empty string.
    pub input: Option<String>,
    /// The instruction that tells the model how to edit the prompt.
    pub instruction: String,
    /// How many edits to generate for the input and instruction. Defaults to 1.
    pub n: Option<u32>,
    /// What sampling temperature to use, between 0 and 2. Higher values like
    /// 0.8 will make the output more random, while lower values like 0.2 will
    /// make it more focused and deterministic. Defaults to 1.
    pub temperature: Option<f32>,
    /// An alternative to sampling with temperature, called nucleus sampling,
    /// where the model considers the results of the tokens with top_p
    /// probability mass. So 0.1 means only the tokens comprising the top 10%
    /// probability mass are considered. Defaults to 1.
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    /// The edited text.
    pub text: String,
    /// The index of the choice in the response.
    pub index: u32,
}

/// Represents the token usage information in the response.
#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    /// The number of tokens used for the prompt.
    pub prompt_tokens: u32,
    /// The number of tokens used for the completion.
    pub completion_tokens: u32,
    /// The total number of tokens used.
    pub total_tokens: u32,
}
