mod api;
mod api_client;
mod common;
mod config_files;
mod powers;
mod prompt;

use anyhow::Context;
use api::{ChatCompletionRequest, ChatCompletionResponse};
use api_client::Client;
use clap::Parser;
use common::undiff;
use config_files::Secrets;
use serde::{Deserialize, Serialize};
use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Once,
};
use xdg::BaseDirectories;

use crate::{
    api::Message,
    prompt::{chat_prefix, fuzzy_undiff},
};
#[derive(Parser)]
#[clap(version, author, about)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    /// Save your openai api key for future use.
    Login,
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
        SubCommand::Login => {
            println!("https://platform.openai.com/account/api-keys");
            let api_key = rpassword::prompt_password("Enter your OpenAI API key:")?;
            Secrets {
                openai_api_key: api_key,
            }
            .save()?;
        }
        SubCommand::Tor {
            selected,
            transform,
        } => {
            let secrets = Secrets::load()?;
            let completion = refactor(selected, transform, &secrets)?;
            print!("{}", completion);
        }
    };

    Ok(())
}

fn refactor(selected: String, transform: String, sc: &Secrets) -> anyhow::Result<String> {
    let client = Client::new(&sc.openai_api_key);
    let mut messages = chat_prefix();
    messages.push(Message::user(&selected));
    messages.push(Message::user(&transform));

    let request = ChatCompletionRequest {
        model: "gpt-4".into(), // don't have access to "gpt-4-32k" yet
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
        functions: None,
    };

    let response = client.request(&request)?;

    log(
        LogEntry {
            inp: request,
            res: response.clone(),
        },
        "logs",
    )?;

    tracing::debug!("response: {}", serde_json::to_string(&response).unwrap());

    let diff = response
        .choices
        .into_iter()
        .next()
        .ok_or(anyhow::anyhow!("No choices returned."))?
        .message
        .try_into_assistant_content()
        .ok_or(anyhow::anyhow!("Assistant tried to call a function."))?;

    tracing::debug!("diff: \n{}", diff);

    let result = match undiff(&selected, &diff) {
        Ok(new) => new,
        Err(err) => {
            log(
                UndiffFailure {
                    selected: selected.clone(),
                    diff: diff.clone(),
                    transform,
                    err: err.to_string(),
                },
                "undiff_failure",
            )?;
            fuzzy_undiff(&selected, &diff, &client, "gpt-3.5-turbo")?
        }
    };

    Ok(result)
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

#[derive(Debug, Serialize, Deserialize)]
struct LogEntry {
    inp: ChatCompletionRequest,
    res: ChatCompletionResponse,
}

#[derive(Debug, Serialize, Deserialize)]
struct UndiffFailure {
    selected: String,
    diff: String,
    transform: String,
    err: String,
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
