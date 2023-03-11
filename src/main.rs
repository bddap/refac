mod api;
mod config_files;
mod pretrain_sample;

use std::time::Duration;

use api::FinetuneInput;
use clap::Parser;
use config_files::Config;

use crate::{
    api::Client,
    config_files::{Secrets, TrainingData},
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
            let _secrets = Secrets::load()?;
            let _conf = Config::load()?;
            dbg!(selected, transform);
            unimplemented!()
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

    Config {
        model_id: result.id.clone(),
    }
    .save()?;

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

    Ok(())
}
