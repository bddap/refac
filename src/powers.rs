//! This file defines the capabilities granted to the llm.
use schemars::JsonSchema;
use serde::Serialize;

struct Context {
    current: String,
}

trait Cabability: JsonSchema + Serialize {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    fn execute(&self, ctx: &Context) -> anyhow::Result<String>;
}

#[derive(Debug, JsonSchema, Serialize)]
struct Replace {
    from: String,
    to: String,
}

impl Cabability for Replace {
    const NAME: &'static str = "replace";
    const DESCRIPTION: &'static str = "replace all occurences of \"from\" with \"to\"";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        if !ctx.current.contains(&self.from) {
            return Err(anyhow::anyhow!(
                "{} not found in {}",
                self.from,
                ctx.current
            ));
        }
        Ok(ctx.current.replace(&self.from, &self.to))
    }
}

#[derive(Debug, JsonSchema, Serialize)]
struct RegexReplace {
    from: String,
    to: String,
}

impl Cabability for RegexReplace {
    const NAME: &'static str = "regex_replace";
    const DESCRIPTION: &'static str =
        "replace all non-overlapping matches of \"from\" in the text with the \"to\"";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        let re = regex::Regex::new(&self.from)?;
        Ok(re.replace_all(&ctx.current, &self.to).to_string())
    }
}

#[derive(Debug, JsonSchema, Serialize)]
struct Note {
    text: String,
}

impl Cabability for Note {
    const NAME: &'static str = "note";
    const DESCRIPTION: &'static str =
        "insert a note to self, use this for chain-of-thought reasoning";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        Ok(ctx.current.clone())
    }
}

#[derive(Debug, JsonSchema, Serialize)]
struct RequestNewCabability {
    name: String,
    description: String,
    schema: serde_json::Value,
    example_implementation: String,
}

impl Cabability for RequestNewCabability {
    const NAME: &'static str = "request_new_capability";
    const DESCRIPTION: &'static str = "request a new capability, the request will be presented to the user who will will implement the function";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        Ok(ctx.current.clone())
    }
}
