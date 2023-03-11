use std::io::Cursor;

use reqwest::blocking::multipart::{Form, Part};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tap::Pipe;

pub struct Client {
    pub client: reqwest::blocking::Client,
    pub token: String,
}

#[derive(Deserialize, Debug)]
pub struct FileUploadResponse {
    pub bytes: usize,
    pub created_at: usize,
    pub filename: String,
    pub id: String,
    pub object: String,
    pub purpose: String,
    pub status: String,
    pub status_details: Option<String>,
}

/// https://platform.openai.com/docs/api-reference/fine-tunes/create
#[derive(Serialize, Deserialize, Debug)]
pub struct FinetuneInput {
    pub training_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_epochs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning_rate_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_loss_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_classification_metrics: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification_n_classes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification_positive_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification_betas: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct FineTuneResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    pub created_at: usize,
    pub events: Vec<FineTuneEvent>,
    pub fine_tuned_model: Option<String>,
    pub hyperparams: FineTuneHyperParams,
    pub organization_id: String,
    pub result_files: Vec<Value>,
    pub status: String,
    pub validation_files: Vec<Value>,
    pub training_files: Vec<FineTuneFile>,
    pub updated_at: usize,
}

#[derive(Deserialize, Debug)]
pub struct FineTuneEvent {
    pub object: String,
    pub created_at: usize,
    pub level: String,
    pub message: String,
}

#[derive(Deserialize, Debug)]
pub struct FineTuneHyperParams {
    pub batch_size: Option<usize>,
    pub learning_rate_multiplier: Option<f64>,
    pub n_epochs: usize,
    pub prompt_loss_weight: f64,
}

#[derive(Deserialize, Debug)]
pub struct FineTuneFile {
    pub id: String,
    pub object: String,
    pub bytes: usize,
    pub created_at: usize,
    pub filename: String,
    pub purpose: String,
}

fn form_part_file(filename: &str, file_content: &[u8]) -> Part {
    let reader = Cursor::new(file_content.to_vec());
    Part::reader(reader).file_name(filename.to_string())
}

impl Client {
    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    pub fn upload(
        &self,
        filename: &str,
        file_content: &[u8],
    ) -> anyhow::Result<FileUploadResponse> {
        let form = Form::new()
            .text("purpose", "fine-tune")
            .part("file", form_part_file(filename, file_content));
        let resp = self
            .client
            .post("https://api.openai.com/v1/files")
            .header("Authorization", self.auth())
            .multipart(form)
            .send()?
            .pipe(err_with_body)?
            .pipe(try_json)?;
        Ok(resp)
    }

    pub fn new(openai_api_key: String) -> Self {
        Client {
            client: reqwest::blocking::Client::new(),
            token: openai_api_key,
        }
    }

    pub fn fine_tune(&self, input: &FinetuneInput) -> anyhow::Result<FineTuneResponse> {
        let resp = self
            .client
            .post("https://api.openai.com/v1/fine-tunes")
            .header("Authorization", self.auth())
            .json(&input)
            .send()?
            .pipe(err_with_body)?
            .pipe(try_json)?;
        Ok(resp)
    }

    pub fn get_fine_tune(&self, id: &str) -> anyhow::Result<FineTuneResponse> {
        let resp = self
            .client
            .get(format!("https://api.openai.com/v1/fine-tunes/{}", id))
            .header("Authorization", self.auth())
            .send()?
            .pipe(err_with_body)?
            .json::<FineTuneResponse>()?;
        Ok(resp)
    }
}

fn err_with_body(resp: reqwest::blocking::Response) -> anyhow::Result<reqwest::blocking::Response> {
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("Error: {}", resp.text()?));
    }
    Ok(resp)
}

/// try to parse as json, if it fails, return a error message with the body
/// and as much debug info as possible
fn try_json<T: DeserializeOwned>(resp: reqwest::blocking::Response) -> anyhow::Result<T> {
    let v = resp.json::<Value>()?;
    serde_json::from_value::<T>(v.clone()).map_err(|e| {
        anyhow::anyhow!(
            "Error: {} while parsing response: {}",
            e,
            serde_json::to_string_pretty(&v).unwrap()
        )
    })
}
