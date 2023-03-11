mod api;
mod config_files;
mod pretrain_sample;
mod tokenizer;

use std::time::Duration;

use api::FinetuneInput;
use clap::Parser;
use config_files::Config;

use crate::{
    api::{Client, CompletionRequest},
    config_files::{Secrets, TrainingData},
    pretrain_sample::Sample,
    tokenizer::count_tokens,
};

#[derive(Parser)]
#[clap(version, author, about)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

// TODO:
//   Just to login and fine-tune automatically when the user runs the program
//   for the first time.
#[derive(Parser)]
enum SubCommand {
    /// Save your openai api key for future use.
    Login,
    /// Create a custom fine-tuned model from the building samples.
    Finetune,
    /// Get it? 'refac tor'. Perform inference.
    /// If no subcommand is provided, this is the default.
    Tor {
        #[clap(short, long)]
        selected: String,
        #[clap(short, long)]
        transform: String,
    },
}

fn main() {
    tracing_subscriber::fmt::init();
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn run() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Login => {
            let api_key = rpassword::prompt_password("Enter your OpenAI API key: ")?;
            Secrets {
                openai_api_key: api_key,
            }
            .save()?;
        }
        SubCommand::Finetune => {
            let secrets = Secrets::load()?;
            let fi = TrainingData::builtin();
            fine_tune(&secrets, &fi)?;
        }
        SubCommand::Tor {
            selected,
            transform,
        } => {
            let secrets = Secrets::load()?;
            let conf = Config::load()?;
            let completion = refactor(&selected, &transform, &secrets, &conf)?;
            println!("{}", completion);
        }
    };

    Ok(())
}

fn fine_tune(secret: &Secrets, td: &TrainingData) -> anyhow::Result<()> {
    let c = Client::new(secret.openai_api_key.to_string());

    let file_contents = td.to_jsonl();
    let resp = c.upload("finetune.jsonl", file_contents.as_bytes())?;
    tracing::info!("Uploaded training data.");

    let fi = FinetuneInput {
        training_file: resp.id,
        validation_file: None,
        model: Some(td.base_model.clone()),
        n_epochs: None,
        batch_size: None,
        learning_rate_multiplier: None,
        prompt_loss_weight: None,
        compute_classification_metrics: None,
        classification_n_classes: None,
        classification_positive_class: None,
        classification_betas: None,
        suffix: Some("refac".to_string()),
    };
    let mut result = c.fine_tune(&fi)?;
    tracing::info!("Started fine-tuning. Model ID: {}", result.id);

    loop {
        tracing::info!("{}...", result.status);
        tracing::debug!("{:#?}", result);
        match result.status.as_str() {
            "succeeded" => break,
            "pending" | "running" => {
                for _ in 0..10 {
                    std::thread::sleep(Duration::from_secs(6));
                    tracing::info!(".");
                }
            }
            _ => {
                Err(anyhow::anyhow!(
                    "Finetuning status went to {}",
                    &result.status
                ))?;
            }
        }
        result = c.get_fine_tune(&result.id)?;
    }

    let model_id = result.fine_tuned_model.ok_or(anyhow::anyhow!(
        "Fine-tuning succeeded but no model ID was returned."
    ))?;
    Config { model_id }.save()?;

    Ok(())
}

fn refactor(
    selected: &str,
    transform: &str,
    sc: &Secrets,
    conf: &Config,
) -> anyhow::Result<String> {
    // Different models have different max tokens so this we'll need to not hardcode
    // this if we want to support other models the current generation of fine-tuneable
    // models are all 2049?
    const MAX_TOKENS: usize = 2049;

    let c = Client::new(sc.openai_api_key.to_string());
    let prompt = Sample::prompt_for(selected, transform);

    let rest = MAX_TOKENS
        .checked_sub(count_tokens(&prompt))
        .ok_or_else(|| anyhow::anyhow!("that prompt is probably too long"))?;

    let resp = c.complete(&CompletionRequest {
        model: conf.model_id.clone(),
        max_tokens: Some(rest),
        prompt,
        temperature: None,
        top_p: None,
        n: None,
        stream: None,
        logprobs: None,
        echo: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
        best_of: None,
        logit_bias: None,
        user: None,
    })?;

    let completion = resp
        .choices
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("the api didn't provide any completions"))?
        .text
        .to_string();

    Ok(completion)
}
