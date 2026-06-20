use std::collections::HashMap;

use anyhow::Result;
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::edit::{self, Edit};

pub struct Seed<'a> {
    pub system: &'a str,
    pub selected: &'a str,
    pub transform: &'a str,
}

pub const SEED_TOOL: &str = "view";
pub const SEED_CALL_ID: &str = "seed_view";

impl Seed<'_> {
    pub fn seed_call_args() -> Value {
        serde_json::json!({})
    }
}

pub fn placeholder_if_empty(field: &str) -> &str {
    if field.is_empty() {
        "(empty)"
    } else {
        field
    }
}

pub struct Ctx<'a> {
    original: &'a str,
}

pub type Reply = std::result::Result<String, String>;

enum Step {
    Continue {
        reply: Reply,
        attempt: Option<Attempt>,
    },
    Finish,
}

impl Step {
    fn reply(reply: Reply) -> Step {
        Step::Continue {
            reply,
            attempt: None,
        }
    }
}

type Handler = Box<dyn Fn(&mut String, &Ctx, Value) -> Result<Step>>;

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Schema,
    run: Handler,
}

impl Tool {
    fn new<A: JsonSchema + DeserializeOwned + 'static>(
        name: &'static str,
        description: &'static str,
        handler: impl Fn(&mut String, &Ctx, A) -> Step + 'static,
    ) -> Tool {
        Tool {
            name,
            description,
            input_schema: schemars::schema_for!(A),
            run: Box::new(move |buf, ctx, args| {
                Ok(handler(buf, ctx, serde_json::from_value(args)?))
            }),
        }
    }
}

#[derive(JsonSchema, serde::Deserialize)]
struct NoArgs {}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new::<Edit>(
            "edit",
            "Replace an exact substring of the selected text. Copy `old` verbatim \
                (whitespace and indentation included); make it long enough to be unique, or set \
                `replace_all`. `new` is the replacement — empty to delete; to insert, include \
                surrounding text in both `old` and `new`. Call this several times in one turn to \
                make several edits.",
            |buf, _ctx, e: Edit| match edit::apply(buf, &e) {
                Ok(next) => {
                    *buf = next;
                    Step::Continue {
                        reply: Ok("ok".into()),
                        attempt: Some(Attempt {
                            edit: e,
                            error: None,
                        }),
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    Step::Continue {
                        reply: Err(msg.clone()),
                        attempt: Some(Attempt {
                            edit: e,
                            error: Some(msg),
                        }),
                    }
                }
            },
        ),
        Tool::new::<NoArgs>(
            "view",
            "Return the current text, with all edits so far applied. Use it to re-anchor if \
                you've lost track of the exact contents.",
            |buf, _ctx, _: NoArgs| Step::reply(Ok(buf.clone())),
        ),
        Tool::new::<NoArgs>(
            "reset",
            "Discard all edits and restore the original selected text. Returns it.",
            |buf, ctx, _: NoArgs| {
                *buf = ctx.original.to_owned();
                Step::reply(Ok(buf.clone()))
            },
        ),
        Tool::new::<NoArgs>(
            "finish",
            "Signal that the transform is complete. refac outputs the current text. Call this \
                when you're done editing.",
            |_buf, _ctx, _: NoArgs| Step::Finish,
        ),
    ]
}

pub struct RawCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

pub struct ToolResult {
    pub id: String,
    pub result: Reply,
}

pub trait Model {
    fn turn(&mut self, results: Vec<ToolResult>) -> Result<Vec<RawCall>>;
}

pub const DEFAULT_MAX_TURNS: usize = 25;

const MAX_CONSECUTIVE_FAILURES: usize = 3;

#[derive(Debug)]
pub struct Attempt {
    pub edit: Edit,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct Outcome {
    pub text: String,
    pub attempts: Vec<Attempt>,
}

pub fn run(model: &mut dyn Model, original: String, max_turns: usize) -> Result<Outcome> {
    let tools = tools();
    let by_name: HashMap<&str, &Tool> = tools.iter().map(|t| (t.name, t)).collect();
    let ctx = Ctx {
        original: &original,
    };

    let mut current = original.clone();
    let mut attempts = Vec::new();
    let mut consecutive_failures = 0;
    let mut pending: Vec<ToolResult> = Vec::new();

    for _ in 0..max_turns {
        let calls = model.turn(std::mem::take(&mut pending))?;
        if calls.is_empty() {
            return Ok(Outcome {
                text: current,
                attempts,
            });
        }

        let mut results = Vec::with_capacity(calls.len());
        let mut edits_attempted = 0;
        let mut edits_failed = 0;

        for RawCall { id, name, args } in calls {
            let step = match by_name.get(name.as_str()) {
                Some(tool) => (tool.run)(&mut current, &ctx, args),
                None => Err(anyhow::anyhow!("unknown tool {name:?}")),
            };

            let (reply, attempt) = match step {
                Ok(Step::Finish) => {
                    return Ok(Outcome {
                        text: current,
                        attempts,
                    })
                }
                Ok(Step::Continue { reply, attempt }) => (reply, attempt),
                Err(err) => (Err(err.to_string()), None),
            };

            if let Some(attempt) = attempt {
                edits_attempted += 1;
                if attempt.error.is_some() {
                    edits_failed += 1;
                }
                attempts.push(attempt);
            }

            results.push(ToolResult { id, result: reply });
        }

        if edits_attempted > 0 && edits_failed == edits_attempted {
            consecutive_failures += 1;
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                anyhow::bail!(
                    "giving up after {consecutive_failures} consecutive turns of failed edits"
                );
            }
        } else {
            consecutive_failures = 0;
        }

        pending = results;
    }

    anyhow::bail!("edit loop hit its {max_turns}-turn limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct ScriptedModel {
        turns: std::vec::IntoIter<Vec<RawCall>>,
        seen: Vec<Vec<ToolResult>>,
    }

    impl ScriptedModel {
        fn new(turns: Vec<Vec<RawCall>>) -> Self {
            ScriptedModel {
                turns: turns.into_iter(),
                seen: Vec::new(),
            }
        }
    }

    impl Model for ScriptedModel {
        fn turn(&mut self, results: Vec<ToolResult>) -> Result<Vec<RawCall>> {
            self.seen.push(results);
            Ok(self.turns.next().unwrap_or_default())
        }
    }

    fn edit_call(id: &str, old: &str, new: &str) -> RawCall {
        RawCall {
            id: id.into(),
            name: "edit".into(),
            args: json!({ "old": old, "new": new }),
        }
    }

    fn call(id: &str, name: &str) -> RawCall {
        RawCall {
            id: id.into(),
            name: name.into(),
            args: json!({}),
        }
    }

    const TURNS: usize = 25;

    #[test]
    fn edit_then_finish() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "Me like", "I like")],
            vec![call("2", "finish")],
        ]);
        let out = run(&mut m, "Me like toast.".into(), TURNS).unwrap().text;
        assert_eq!(out, "I like toast.");
    }

    #[test]
    fn empty_selection_placeholder_is_editable_into_generated_text() {
        let seeded = placeholder_if_empty("");
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "(empty)", "fn main() {}")],
            vec![call("2", "finish")],
        ]);
        let out = run(&mut m, seeded.to_string(), TURNS).unwrap().text;
        assert_eq!(out, "fn main() {}");
    }

    #[test]
    fn parallel_edits_in_one_turn() {
        let mut m = ScriptedModel::new(vec![vec![
            edit_call("1", "one", "1"),
            edit_call("2", "two", "2"),
            call("3", "finish"),
        ]]);
        let out = run(&mut m, "one two".into(), TURNS).unwrap().text;
        assert_eq!(out, "1 2");
    }

    #[test]
    fn natural_done_without_finish() {
        let mut m = ScriptedModel::new(vec![vec![edit_call("1", "a", "b")], vec![]]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
    }

    #[test]
    fn failed_edit_is_reported_then_recovered() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "nope", "x")],
            vec![edit_call("2", "a", "b"), call("3", "finish")],
        ]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
        let err = m.seen[1][0].result.as_ref().unwrap_err();
        assert!(err.contains("could not find"));
    }

    #[test]
    fn view_returns_current_buffer() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "a", "b")],
            vec![call("2", "view")],
            vec![call("3", "finish")],
        ]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
        assert_eq!(m.seen[2][0].result, Ok("b".to_string()));
    }

    #[test]
    fn reset_restores_original() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "a", "b")],
            vec![call("2", "reset")],
            vec![call("3", "finish")],
        ]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "a");
        assert_eq!(m.seen[2][0].result, Ok("a".to_string()));
    }

    #[test]
    fn unknown_tool_is_an_error_result_not_a_crash() {
        let mut m = ScriptedModel::new(vec![
            vec![call("1", "frobnicate")],
            vec![call("2", "finish")],
        ]);
        let out = run(&mut m, "x".into(), TURNS).unwrap().text;
        assert_eq!(out, "x");
        let err = m.seen[1][0].result.as_ref().unwrap_err();
        assert!(err.contains("unknown tool"));
    }

    #[test]
    fn aborts_after_consecutive_failures() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "nope", "x")],
            vec![edit_call("2", "nope", "x")],
            vec![edit_call("3", "nope", "x")],
        ]);
        let err = run(&mut m, "a".into(), TURNS).unwrap_err();
        assert!(err.to_string().contains("consecutive"));
    }

    #[test]
    fn pure_view_turns_do_not_count_as_failures() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "nope", "x")],
            vec![call("2", "view")],
            vec![edit_call("3", "nope", "x")],
            vec![edit_call("4", "a", "b"), call("5", "finish")],
        ]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
    }

    #[test]
    fn hits_turn_limit() {
        let turns = (0..30)
            .map(|i| vec![call(&i.to_string(), "view")])
            .collect();
        let mut m = ScriptedModel::new(turns);
        let err = run(&mut m, "x".into(), 5).unwrap_err();
        assert!(err.to_string().contains("limit"));
    }
}
