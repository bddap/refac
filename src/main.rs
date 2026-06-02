mod agent;
mod anthropic;
mod api;
mod backend;
mod config_files;
mod edit;
mod openai;
mod prompt;

use anyhow::Context;
use api::Message;
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
    let provider = config.provider(sc);
    let model = config.model(provider);

    let mut seed = prompt::edit_prefix();
    seed.push(Message::user(vec![selected.clone(), transform.clone()]));
    let tools = agent::tools();
    let mut model_agent = backend::resolve_agent(provider, &model, sc, &seed, &tools)?;

    // Log each edit attempt so we can see how often the model's `old` misses —
    // the failure-rate signal.
    let mut on_edit = |o: agent::EditOutcome| {
        let _ = log(
            EditLog {
                provider,
                model: model.clone(),
                old: o.edit.old.clone(),
                new: o.edit.new.clone(),
                error: o.error.map(|e| e.to_string()),
            },
            "edits",
        );
    };
    let output = agent::run_with(
        model_agent.as_mut(),
        selected.clone(),
        &agent::Limits::default(),
        &mut on_edit,
    )?;

    log(
        LogEntry {
            provider,
            model,
            selected,
            transform,
            output: output.clone(),
        },
        "logs",
    )?;

    Ok(output)
}

/// One `edit` tool attempt, logged to `edits.jsonl`. `error` is `None` on success;
/// the rate of `Some` is how often the model's `old` failed to match.
#[derive(Debug, Serialize)]
struct EditLog {
    provider: Provider,
    model: String,
    old: String,
    new: String,
    error: Option<String>,
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
    provider: Provider,
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
