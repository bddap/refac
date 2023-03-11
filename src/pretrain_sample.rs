// IDEA: generating many samples is time consuming, perhaps we can get codex or
//       chatgpt to generate a bunch of samples that can be manually vetted

// IDEA: try getting completions from a gpt3.5 model without any training

// TODO: can we train on negetive examples?

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Sample {
    selected: String,
    transform: String,
    pub result: String,
    #[serde(
        default = "Correctness::correct",
        skip_serializing_if = "Correctness::is_correct"
    )]
    correct: Correctness,
}

impl Sample {
    /// TODO: improve prompts using advice from openai
    ///       https://platform.openai.com/docs/guides/fine-tuning/data-formatting
    pub fn prompt_for(selected: &str, transform: &str) -> String {
        Sample {
            selected: selected.to_string(),
            transform: transform.to_string(),
            result: String::new(),
            correct: Correctness::correct(),
        }
        .prompt()
    }

    pub fn prompt(&self) -> String {
        let prompt = serde_json::json!({
            "original_text": self.selected,
            "transformation_instructions": self.transform,
        });
        serde_json::to_string(&prompt).unwrap() + "\nimproved_text: "
    }

    pub fn completion(&self) -> &str {
        &self.result
    }

    pub fn is_correct(&self) -> bool {
        self.correct.is_correct()
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Correctness {
    Correct,
    Incorrect,
    Unknown,
}

impl Correctness {
    fn correct() -> Self {
        Correctness::Correct
    }

    fn is_correct(&self) -> bool {
        matches!(self, Correctness::Correct)
    }
}
