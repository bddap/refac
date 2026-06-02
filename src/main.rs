mod anthropic;
mod api;
mod api_client;
mod config_files;
mod prompt;

use anyhow::Context;
use api::{field_or_placeholder, ChatCompletionRequest, Message, OpenAiMessage};
use api_client::Client;
use clap::Parser;
use config_files::{Config, Provider, Secrets};
use serde::Serialize;
use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Once,
};
use xdg::BaseDirectories;

use crate::prompt::chat_prefix;

#[derive(Parser)]
#[clap(version, author, about)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    /// Save your API key for future use. Pass `--provider`, or pick one interactively.
    Login {
        #[clap(long)]
        provider: Option<Provider>,
    },
    /// Apply the instructions encoded in `transform` to the text in `selected`.
    /// Get it? 'refac tor'
    Tor { selected: String, transform: String },
}

fn main() {
    tracing_subscriber::fmt::init();
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{:?}", e);
            std::process::exit(1);
        }
    }
}

fn run() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Login { provider } => {
            let mut secrets = Secrets::load().unwrap_or_default();
            let provider = match provider {
                Some(p) => p,
                None => {
                    let choices = [Provider::Anthropic, Provider::Openai];
                    let labels: Vec<String> = choices.iter().map(|p| format!("{p:?}")).collect();
                    let idx = dialoguer::Select::new()
                        .with_prompt("Which provider?")
                        .items(&labels)
                        .default(0)
                        .interact()?;
                    choices[idx]
                }
            };
            match provider {
                Provider::Anthropic => {
                    println!("https://console.anthropic.com/settings/keys");
                    let api_key = rpassword::prompt_password("Enter your Anthropic API key:")?;
                    secrets.anthropic_api_key = Some(api_key);
                }
                Provider::Openai => {
                    println!("https://platform.openai.com/account/api-keys");
                    let api_key = rpassword::prompt_password("Enter your OpenAI API key:")?;
                    secrets.openai_api_key = Some(api_key);
                }
            }
            secrets.save()?;
        }
        SubCommand::Tor {
            selected,
            transform,
        } => {
            let secrets = Secrets::load()?;
            let config = Config::load()?;
            let completion = refactor(selected, transform, &secrets, &config)?;
            print!("{}", completion);
        }
    };

    Ok(())
}

fn refactor(
    selected: String,
    transform: String,
    sc: &Secrets,
    config: &Config,
) -> anyhow::Result<String> {
    let mut messages = chat_prefix();
    messages.push(Message::user(vec![selected.clone(), transform.clone()]));

    let provider = config.provider(sc);
    let model = config.model(provider);

    let output = match provider {
        Provider::Anthropic => {
            let key = sc.anthropic_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "No Anthropic API key found. Set ANTHROPIC_API_KEY or run 'refac login'."
                )
            })?;
            anthropic::complete(key, &model, &messages)?
        }
        Provider::Openai => {
            let key = sc.openai_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "No OpenAI API key found. Set OPENAI_API_KEY or run 'refac login'."
                )
            })?;
            openai_complete(key, &model, &messages)?
        }
    };

    log(
        LogEntry {
            provider: format!("{:?}", provider),
            model,
            selected,
            transform,
            output: output.clone(),
        },
        "logs",
    )?;

    Ok(output)
}

fn openai_complete(api_key: &str, model: &str, messages: &[Message]) -> anyhow::Result<String> {
    let client = Client::new(api_key);

    // OpenAI takes one string per message; concatenating a turn's fields would
    // blur the selected text into the transform with no reliable boundary, so
    // emit each field as its own message. `cache` has no OpenAI equivalent.
    let messages: Vec<OpenAiMessage> = messages
        .iter()
        .flat_map(|m| {
            m.fields.iter().map(move |f| OpenAiMessage {
                role: m.role,
                content: field_or_placeholder(f).to_string(),
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

fn log_location(title: &str) -> anyhow::Result<PathBuf> {
    let bd = BaseDirectories::with_prefix("refac")?;
    let ret = bd.get_data_file(format!("{title}.jsonl"));

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        tracing::debug!("Logging to {:?}", bd.get_data_home());
    });

    // ensure the parent directory exists
    ret.parent().map(create_dir_all).transpose()?;

    Ok(ret)
}

#[derive(Debug, Serialize)]
struct LogEntry {
    provider: String,
    model: String,
    selected: String,
    transform: String,
    output: String,
}

fn log<T: Serialize>(t: T, title: &str) -> anyhow::Result<()> {
    fn inner<T: Serialize>(t: T, title: &str) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_location(title)?)
            .context("opening log file")?;
        let line = serde_json::to_string(&t)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    inner(t, title).with_context(|| format!("failed to log {}", title))
}
