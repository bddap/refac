mod config_files;
mod pretrain_sample;

use clap::Parser;
use config_files::Config;
use reqwest::blocking::{
    multipart::{Form, Part},
    Body, Client,
};
use serde::Deserialize;

use crate::config_files::{FinetuneInput, Secrets};

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
    /// Create a custom fine-tuned model from the building samples.
    Finetune,
    /// Get it? 'refac tor'. Perform inference.
    Tor { selected: String, transform: String },
}

fn main() -> anyhow::Result<()> {
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
    let c = Client::new();

    let auth = format!("Bearer {}", secret.openai_api_key);
    let file_contents = fi.to_jsonl();

    // curl https://api.openai.com/v1/files \
    //    -H "Authorization: Bearer YOUR_API_KEY" \
    //    -F purpose="fine-tune" \
    //    -F file='@mydata.jsonl'

    let part = form_part_file("finetune.jsonl", &file_contents);

    let resp = c
        .post("https://api.openai.com/v1/files")
        .header("Authorization", auth)
        .multipart(Form::new().text("purpose", "fine-tune").part("file", part))
        .send()?;

    #[derive(Deserialize, Debug)]
    struct Response {
        bytes: usize,
        created_at: usize,
        filename: String,
        id: String,
        object: String,
        purpose: String,
        status: String,
        status_details: Option<String>,
    }

    let resp = resp.json::<Response>()?;
    dbg!(&resp);

    // let prompt_length = prompt.len() as u32;
    // if prompt_length >= MAX_TOKENS {
    //     return Err(format!(
    //         "Prompt cannot exceed length of {} characters",
    //         MAX_TOKENS - 1
    //     )
    //     .into());
    // }

    // let p = Prompt {
    //     max_tokens: MAX_TOKENS - prompt_length,
    //     model: String::from(OPENAI_MODEL),
    //     prompt,
    //     temperature: TEMPERATURE,
    // };

    // let mut auth = String::from("Bearer ");
    // auth.push_str(&self.api_key);

    // let mut headers = HeaderMap::new();
    // headers.insert("Authorization", HeaderValue::from_str(auth.as_str())?);
    // headers.insert("Content-Type", HeaderValue::from_str("application/json")?);

    // let body = serde_json::to_string(&p)?;

    // let client = Client::new();
    // let mut res = client.post(&self.url).body(body).headers(headers).send()?;

    // let mut response_body = String::new();
    // res.read_to_string(&mut response_body)?;
    // let json_object: Value = from_str(&response_body)?;
    // let answer = json_object["choices"][0]["text"].as_str();

    // match answer {
    //     Some(a) => Ok(String::from(a)),
    //     None => {
    //         util::pretty_print(&response_body, "json");
    //         Err("JSON parse error".into())
    //     }
    // }
    unimplemented!()
}

fn form_part_file(filename: &str, file_content: &str) -> Part {
    let reader = std::io::Cursor::new(file_content.to_string());
    Part::reader(reader).file_name(filename.to_string())
}
