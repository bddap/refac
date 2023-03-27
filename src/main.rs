mod api;
mod api_client;
mod config_files;
mod prompt;

use anyhow::Context;
use api::{ChatCompletionRequest, ChatCompletionResponse};
use api_client::Client;
use clap::Parser;
use config_files::Secrets;
use serde::{Deserialize, Serialize};
use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
};
use xdg::BaseDirectories;

use crate::{api::Message, prompt::chat_prefix};

#[derive(Parser)]
#[clap(version, author, about)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
    #[clap(long, default_value = "false")]
    /// Slightly modify refac's personality.
    sass: bool,
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
            let completion = refactor(selected, transform, &secrets, opts.sass)?;
            print!("{}", completion);
        }
    };

    Ok(())
}

fn refactor(
    selected: String,
    transform: String,
    sc: &Secrets,
    sassy: bool,
) -> anyhow::Result<String> {
    let client = Client::new(&sc.openai_api_key);
    let mut messages = chat_prefix(sassy);
    messages.push(Message::user(selected));
    messages.push(Message::user(transform));

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
    };

    let response = client.request(&request)?;

    LogEntry {
        inp: request,
        res: response.clone(),
    }
    .log()
    .context("failed to log")?;

    let result = response
        .choices
        .into_iter()
        .next()
        .ok_or(anyhow::anyhow!("No choices returned."))?
        .message
        .content;

    Ok(result)
}

fn log_location() -> anyhow::Result<PathBuf> {
    let ret = BaseDirectories::with_prefix("refac")?.get_data_file("logs.jsonl");

    // ensure the parent directory exists
    ret.parent().map(create_dir_all).transpose()?;

    Ok(ret)
}

#[derive(Debug, Serialize, Deserialize)]
struct LogEntry {
    inp: ChatCompletionRequest,
    res: ChatCompletionResponse,
}

impl LogEntry {
    fn log(&self) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_location()?)
            .context("opening log file")?;
        let line = serde_json::to_string(self)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }
}
