//! This file defines the capabilities granted to the llm.
use schemars::{gen::SchemaGenerator, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::api::{FunctionCall, FunctionSpec};

struct Context {
    current: String,
}

trait Capability: JsonSchema + Serialize {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    fn execute(&self, ctx: &Context) -> anyhow::Result<String>;
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
struct Replace {
    from: String,
    to: String,
}

impl Capability for Replace {
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

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
struct RegexReplace {
    from: String,
    to: String,
}

impl Capability for RegexReplace {
    const NAME: &'static str = "regex_replace";
    const DESCRIPTION: &'static str =
        "replace all non-overlapping matches of \"from\" in the text with the \"to\"";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        let re = regex::RegexBuilder::new(&self.from)
            .dot_matches_new_line(true)
            .build()?;
        Ok(re.replace_all(&ctx.current, &self.to).to_string())
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
struct Note(String);

impl Capability for Note {
    const NAME: &'static str = "note";
    const DESCRIPTION: &'static str =
        "insert a note to self, use this for chain-of-thought reasoning";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        Ok(ctx.current.clone())
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
struct RequestNewCabability {
    name: String,
    description: String,
    schema: serde_json::Value,
    example_implementation: String,
}

impl Capability for RequestNewCabability {
    const NAME: &'static str = "request_new_capability";
    const DESCRIPTION: &'static str = "request a new capability, the request will be presented to the user who will will implement the function";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        Ok(ctx.current.clone())
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
struct Prepend(String);

impl Capability for Prepend {
    const NAME: &'static str = "prepend";
    const DESCRIPTION: &'static str = "prepend the string to the current string";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        Ok(format!("{}{}", self.0, ctx.current))
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
struct Append(String);

impl Capability for Append {
    const NAME: &'static str = "append";
    const DESCRIPTION: &'static str = "append the string to the current string";
    fn execute(&self, ctx: &Context) -> anyhow::Result<String> {
        Ok(format!("{}{}", ctx.current, self.0))
    }
}

fn function_spec<T: Capability>() -> FunctionSpec {
    let mut schema_generator = SchemaGenerator::default();
    let schema = T::json_schema(&mut schema_generator);
    FunctionSpec {
        name: T::NAME.to_owned(),
        description: T::DESCRIPTION.to_owned(),
        params: [schema].to_vec(),
    }
}

struct Description {
    spec: FunctionSpec,
    run: fn(Value, &Context) -> anyhow::Result<String>,
}

impl Description {
    fn describe<T: Capability + DeserializeOwned>() -> Self {
        Self {
            spec: function_spec::<T>(),
            run: |params, ctx| {
                let params: T = serde_json::from_value(params)?;
                params.execute(ctx)
            },
        }
    }
}

#[lazy_fn::lazy_fn]
fn descriptions() -> Vec<Description> {
    [
        Description::describe::<Replace>(),
        Description::describe::<RegexReplace>(),
        Description::describe::<Note>(),
        Description::describe::<RequestNewCabability>(),
        Description::describe::<Prepend>(),
        Description::describe::<Append>(),
    ]
    .into()
}

#[lazy_fn::lazy_fn]
pub fn function_specs() -> Vec<FunctionSpec> {
    descriptions().iter().map(|d| d.spec.clone()).collect()
}

pub fn execute(call: FunctionCall, current: String) -> anyhow::Result<String> {
    let ctx = Context { current };
    let desc = descriptions()
        .iter()
        .find(|desc| desc.spec.name == call.name)
        .ok_or_else(|| anyhow::anyhow!("unknown function {}", call.name))?;
    let arguments: Value = serde_json::from_str(&call.arguments)?;
    (desc.run)(arguments, &ctx)
}
