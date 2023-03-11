use std::io::Cursor;

use reqwest::blocking::multipart::{Form, Part};
use serde::Deserialize;

pub struct Client {
    pub client: reqwest::blocking::Client,
    pub token: String,
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
            .json::<FileUploadResponse>()?;
        Ok(resp)
    }

    pub fn new(openai_api_key: String) -> Self {
        Client {
            client: reqwest::blocking::Client::new(),
            token: openai_api_key,
        }
    }

    pub fn fine_tune(
        &self,
        file_id: String,
        base_model: String,
    ) -> anyhow::Result<FineTuneResponse> {
        todo!()
    }
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

#[derive(Deserialize, Debug)]
pub struct FineTuneResponse {
    pub id: String,
    pub object: String,
    pub status: String,
    pub status_details: Option<String>,
}

/// https://platform.openai.com/docs/api-reference/fine-tunes/create
#[derive(Deserialize, Debug)]
pub struct FintuneInput {
    pub training_file: String,
    pub validation_file: Option<String>,
    pub model: Option<String>,
    pub n_epochs: Option<usize>,
    pub batch_size: Option<usize>,
    pub learning_rate_multiplier: Option<f64>,
    pub prompt_loss_weight: Option<f64>,
    pub compute_classification_metrics: Option<bool>,
    pub classification_n_classes: Option<usize>,
    pub classification_positive_class: Option<String>,
    pub classification_betas: Option<Vec<f64>>,
    pub suffix: Option<String>,
}

fn form_part_file(filename: &str, file_content: &[u8]) -> Part {
    let reader = Cursor::new(file_content.to_vec());
    Part::reader(reader).file_name(filename.to_string())
}
