mod api;
mod api_client;
mod config_files;

use api::{EditRequest, EditResponse};
use api_client::Client;
use clap::Parser;
use config_files::Secrets;

#[derive(Parser)]
#[clap(version, author, about)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

// TODO:
//   Just to login automatically when the user runs the program
//   for the first time.
#[derive(Parser)]
enum SubCommand {
    /// Save your openai api key for future use.
    Login,
    /// Get it? 'refac tor'. Perform inference.
    /// If no subcommand is provided, this is the default.
    Tor { selected: String, transform: String },
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
    let c = Client::new(&sc.openai_api_key);

    let req = EditRequest {
        model: "text-davinci-edit-001".into(),
        input: Some(selected),
        instruction: transform,
        n: None,
        temperature: None,
        top_p: None,
    };

    let resp: EditResponse = c.request(&req)?;

    tracing::debug!("Token {:?}", resp.usage);

    let completion = resp
        .choices
        .into_iter()
        .next()
        .ok_or(anyhow::anyhow!("No choices returned."))?
        .text;

    Ok(completion)
}
