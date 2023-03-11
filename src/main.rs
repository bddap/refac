mod api;
mod config_files;
mod pretrain_sample;

use clap::Parser;
use config_files::Config;

use crate::{
    api::Client,
    config_files::{FinetuneInput, Secrets},
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
            let fi = FinetuneInput::builtin();
            let config = fine_tune(&secrets, &fi)?;
            config.save()?;
        }
        SubCommand::Tor {
            selected,
            transform,
        } => {
            let secrets = Secrets::load()?;
            let conf = Config::load()?;
            dbg!(selected, transform);
            unimplemented!()
        }
    };

    Ok(())
}

fn fine_tune(secret: &Secrets, fi: &FinetuneInput) -> anyhow::Result<Config> {
    let c = Client::new(secret.openai_api_key.to_string());

    let file_contents = fi.to_jsonl();
    let resp = c.upload("finetune.jsonl", file_contents.as_bytes())?;
    // let result = c.fine_tune(resp.id, fi.base_model)?;
    // dbg!(&resp);

    unimplemented!()
}
