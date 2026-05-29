mod anthropic;
mod api;
mod api_client;
mod config_files;
mod prompt;

use anyhow::Context;
use api::{ChatCompletionRequest, Message};
use api_client::Client;
use clap::Parser;
use config_files::{Config, EditMode, Provider, Secrets};
use serde::Serialize;
use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Once,
};
use xdg::BaseDirectories;

use crate::anthropic::Edit;
use crate::prompt::{chat_prefix, edit_prefix};

#[derive(Parser)]
#[clap(version, author, about)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    /// Save your API key for future use. Defaults to the configured provider
    /// (Anthropic unless overridden); pass --provider to be explicit.
    Login {
        /// Which provider's key to save: "anthropic" or "openai".
        #[clap(long)]
        provider: Option<String>,
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
            // --provider overrides config for this invocation; else use the
            // configured provider (Anthropic by default).
            let provider = match provider.as_deref() {
                None => Config::load()?.provider,
                Some("anthropic") => Provider::Anthropic,
                Some("openai") => Provider::Openai,
                Some(other) => {
                    anyhow::bail!("unknown --provider {other:?} (expected anthropic|openai)")
                }
            };
            let mut secrets = Secrets::load().unwrap_or_default();
            match provider {
                Provider::Anthropic => {
                    println!("Saving an Anthropic API key. (https://console.anthropic.com/settings/keys)");
                    let api_key = rpassword::prompt_password("Enter your Anthropic API key:")?;
                    secrets.anthropic_api_key = Some(api_key);
                }
                Provider::Openai => {
                    println!("Saving an OpenAI API key. (https://platform.openai.com/account/api-keys)");
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
    let model = config.model();

    let output = match config.provider {
        Provider::Anthropic => {
            let key = sc.anthropic_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "No Anthropic API key found. Set ANTHROPIC_API_KEY or run 'refac login'."
                )
            })?;
            match config.edit_mode {
                EditMode::Tool => {
                    // Model returns structured edits via a tool call; apply them
                    // to the original text instead of re-emitting the whole thing.
                    let mut messages = edit_prefix();
                    messages.push(Message::user(&selected));
                    messages.push(Message::user(&transform));
                    let edits =
                        anthropic::request_edits(key, &model, config.max_tokens, &messages)?;
                    apply_edits(&selected, &edits)?
                }
                EditMode::Rewrite => {
                    let mut messages = chat_prefix();
                    messages.push(Message::user(&selected));
                    messages.push(Message::user(&transform));
                    anthropic::complete(key, &model, config.max_tokens, &messages)?
                }
            }
        }
        Provider::Openai => {
            // OpenAI path always rewrites (tool-edit mode is Anthropic-only).
            let key = sc.openai_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "No OpenAI API key found. Set OPENAI_API_KEY or run 'refac login'."
                )
            })?;
            let mut messages = chat_prefix();
            messages.push(Message::user(&selected));
            messages.push(Message::user(&transform));
            openai_complete(key, &model, messages)?
        }
    };

    log(
        LogEntry {
            provider: format!("{:?}", config.provider),
            model,
            selected,
            transform,
            output: output.clone(),
        },
        "logs",
    )?;

    Ok(output)
}

fn openai_complete(api_key: &str, model: &str, messages: Vec<Message>) -> anyhow::Result<String> {
    let client = Client::new(api_key);

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

/// Apply exact-substring edits to `text`. Every `old` is resolved against the
/// ORIGINAL text (not a progressively-mutated buffer), so edits are independent
/// of each other and of ordering. An edit is rejected — failing the whole
/// refactor rather than silently corrupting the buffer — if its `old` is empty,
/// missing, ambiguous (occurs more than once), or overlaps another edit.
fn apply_edits(text: &str, edits: &[Edit]) -> anyhow::Result<String> {
    // Resolve each edit to a byte range in the original text.
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for e in edits {
        if e.old.is_empty() {
            return Err(anyhow::anyhow!(
                "edit has an empty `old`; use a unique anchor substring instead"
            ));
        }
        let start = text
            .find(&e.old)
            .ok_or_else(|| anyhow::anyhow!("edit target not found in text: {:?}", e.old))?;
        let end = start + e.old.len();
        if text[end..].contains(&e.old) {
            return Err(anyhow::anyhow!(
                "edit target is not unique in text: {:?} (use a longer, unique anchor)",
                e.old
            ));
        }
        ranges.push((start, end, e.new.as_str()));
    }

    // Reject overlapping edits.
    ranges.sort_by_key(|r| r.0);
    for w in ranges.windows(2) {
        if w[0].1 > w[1].0 {
            return Err(anyhow::anyhow!("edits overlap in the text; refusing to apply"));
        }
    }

    // Apply right-to-left so earlier byte offsets stay valid.
    let mut out = text.to_string();
    for (start, end, new) in ranges.into_iter().rev() {
        out.replace_range(start..end, new);
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> Edit {
        Edit {
            old: old.to_string(),
            new: new.to_string(),
        }
    }

    #[test]
    fn applies_multiple_edits() {
        let out = apply_edits(
            "Me like toast.",
            &[edit("Me like", "I like"), edit("toast", "bread")],
        )
        .unwrap();
        assert_eq!(out, "I like bread.");
    }

    #[test]
    fn insert_via_anchor_and_delete_via_empty() {
        let out = apply_edits("fn main() {}", &[edit("{}", "{\n    // hi\n}")]).unwrap();
        assert_eq!(out, "fn main() {\n    // hi\n}");
        let out = apply_edits("hello world", &[edit(" world", "")]).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn missing_target_errors() {
        let err = apply_edits("abc", &[edit("xyz", "q")]).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn rejects_ambiguous_old() {
        let err = apply_edits("a a", &[edit("a", "b")]).unwrap_err();
        assert!(err.to_string().contains("not unique"));
    }

    #[test]
    fn rejects_empty_old() {
        let err = apply_edits("abc", &[edit("", "x")]).unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn rejects_overlapping_edits() {
        let err = apply_edits("abcd", &[edit("abc", "X"), edit("bcd", "Y")]).unwrap_err();
        assert!(err.to_string().contains("overlap"));
    }

    #[test]
    fn edits_resolve_against_original_not_mutated_buffer() {
        // edit 1 introduces "foo"; edit 2 must target the ORIGINAL "foo" only,
        // not the one edit 1 created. Both resolve against the original input.
        let out = apply_edits("foo bar", &[edit("bar", "foo"), edit("foo", "baz")]).unwrap();
        assert_eq!(out, "baz foo");
    }
}
