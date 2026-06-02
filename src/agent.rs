//! The edit-mode loop: the model drives a small session over the selected text
//! by calling tools (`edit`, `view`, `reset`, `finish`), refac applies each and
//! feeds the result back, until the model finishes or a guard trips.
//!
//! This module is provider-agnostic and IO-free. A [`Model`] is one turn of
//! "send the conversation + tools, get back the tool calls"; the real providers
//! implement it over their wire formats, and tests implement it with a script.

use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};

use crate::edit::{self, Edit, EditError};

/// A tool exposed to the model: its name, one-line purpose, and JSON-Schema for
/// the arguments. Providers translate these into their own tool-definition shape.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// The tools refac offers in edit mode. `edit` does the work; the other three
/// keep the model oriented and let it end cleanly.
pub fn tools() -> Vec<ToolSpec> {
    let no_args = || json!({ "type": "object", "properties": {} });
    vec![
        ToolSpec {
            name: "edit",
            description: "Replace an exact substring of the selected text. Copy `old` verbatim \
                (whitespace and indentation included); make it long enough to be unique, or set \
                `replace_all`. `new` is the replacement — empty to delete; to insert, include \
                surrounding text in both `old` and `new`. Call this several times in one turn to \
                make several edits.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "old": { "type": "string", "description": "exact text to replace" },
                    "new": { "type": "string", "description": "replacement text" },
                    "replace_all": { "type": "boolean", "description": "replace every occurrence" }
                },
                "required": ["old", "new"]
            }),
        },
        ToolSpec {
            name: "view",
            description: "Return the current text, with all edits so far applied. Use it to \
                re-anchor if you've lost track of the exact contents.",
            input_schema: no_args(),
        },
        ToolSpec {
            name: "reset",
            description: "Discard all edits and restore the original selected text. Returns it.",
            input_schema: no_args(),
        },
        ToolSpec {
            name: "finish",
            description: "Signal that the transform is complete. refac outputs the current text. \
                Call this when you're done editing.",
            input_schema: no_args(),
        },
    ]
}

/// A tool call as it comes off the wire, before refac knows it's valid.
pub struct RawCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// A parsed, understood tool call.
enum Action {
    Edit(Edit),
    View,
    Reset,
    Finish,
}

fn parse(name: &str, args: Value) -> Result<Action> {
    match name {
        "edit" => Ok(Action::Edit(serde_json::from_value(args)?)),
        "view" => Ok(Action::View),
        "reset" => Ok(Action::Reset),
        "finish" => Ok(Action::Finish),
        other => anyhow::bail!("unknown tool {other:?}"),
    }
}

/// What refac sends back for one tool call.
pub struct ToolResult {
    pub id: String,
    pub content: String,
    pub is_error: bool,
}

/// One assistant turn, abstracted over the provider. `results` carries the tool
/// results from the previous turn's calls (empty on the first turn); the impl
/// threads them into the conversation, runs one round-trip, and returns this
/// turn's tool calls (empty = the model ended its turn without calling one, i.e.
/// a natural "done"). Folding "answer the previous calls" and "take the next
/// turn" into one step makes it impossible to advance without supplying results
/// for every outstanding call — which both wire protocols require.
pub trait Model {
    fn turn(&mut self, results: Vec<ToolResult>) -> Result<Vec<RawCall>>;
}

/// Default cap on assistant turns.
pub const DEFAULT_MAX_TURNS: usize = 25;

/// Give up after this many consecutive turns in which every edit failed — the
/// model is stuck and burning tokens.
const MAX_CONSECUTIVE_FAILURES: usize = 3;

/// One `edit` attempt and whether it landed — the per-edit failure-rate signal
/// the caller logs.
#[derive(Debug)]
pub struct Attempt {
    pub edit: Edit,
    pub error: Option<EditError>,
}

/// What the loop produced: the final text and every edit attempt along the way.
#[derive(Debug)]
pub struct Outcome {
    pub text: String,
    pub attempts: Vec<Attempt>,
}

/// Run the edit loop over `original`. `max_turns` caps assistant turns so
/// `view`/`reset` can't spin forever.
pub fn run(model: &mut dyn Model, original: String, max_turns: usize) -> Result<Outcome> {
    let mut current = original.clone();
    let mut attempts = Vec::new();
    let mut consecutive_failures = 0;
    let mut pending: Vec<ToolResult> = Vec::new();

    for _ in 0..max_turns {
        let calls = model.turn(std::mem::take(&mut pending))?;
        if calls.is_empty() {
            return Ok(Outcome { text: current, attempts }); // natural "done"
        }

        let mut results = Vec::with_capacity(calls.len());
        let mut edits_attempted = 0;
        let mut edits_failed = 0;

        for call in calls {
            let RawCall { id, name, args } = call;
            match parse(&name, args) {
                Ok(Action::Finish) => return Ok(Outcome { text: current, attempts }),
                Ok(Action::View) => results.push(ok(id, current.clone())),
                Ok(Action::Reset) => {
                    current = original.clone();
                    results.push(ok(id, current.clone()));
                }
                Ok(Action::Edit(e)) => {
                    edits_attempted += 1;
                    let error = match edit::apply(&current, &e) {
                        Ok(next) => {
                            current = next;
                            results.push(ok(id, "ok".into()));
                            None
                        }
                        Err(err) => {
                            edits_failed += 1;
                            results.push(err_result(id, err.to_string()));
                            Some(err)
                        }
                    };
                    attempts.push(Attempt { edit: e, error });
                }
                Err(err) => results.push(err_result(id, err.to_string())),
            }
        }

        // A turn "fails" only if it tried to edit and every edit missed; a turn
        // of pure `view`/`reset` shouldn't count against the model.
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

        // Hand these to the model on the next turn (one result per call).
        pending = results;
    }

    anyhow::bail!("edit loop hit its {max_turns}-turn limit")
}

/// A blocking HTTP client with refac's standard timeout, shared by the provider
/// agents.
pub fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60 * 4))
        .build()
        .expect("building HTTP client")
}

fn ok(id: String, content: String) -> ToolResult {
    ToolResult {
        id,
        content,
        is_error: false,
    }
}

fn err_result(id: String, content: String) -> ToolResult {
    ToolResult {
        id,
        content,
        is_error: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model driven by a canned script: each entry is the tool calls for one
    /// turn. It records the results refac sends back so tests can assert on them.
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
            // `results` are the previous turn's tool results, so `seen[i]` holds
            // the results the model received entering turn `i` (seen[0] is empty).
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
        // second turn has no calls → loop ends with the current buffer.
        let mut m = ScriptedModel::new(vec![vec![edit_call("1", "a", "b")], vec![]]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
    }

    #[test]
    fn failed_edit_is_reported_then_recovered() {
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "nope", "x")], // misses
            vec![edit_call("2", "a", "b"), call("3", "finish")],
        ]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
        // refac told the model the first edit failed (delivered entering turn 1).
        assert!(m.seen[1][0].is_error);
        assert!(m.seen[1][0].content.contains("could not find"));
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
        // view ran in turn 1; its result reaches the model entering turn 2.
        assert_eq!(m.seen[2][0].content, "b");
        assert!(!m.seen[2][0].is_error);
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
        // reset ran in turn 1; its result reaches the model entering turn 2.
        assert_eq!(m.seen[2][0].content, "a");
    }

    #[test]
    fn unknown_tool_is_an_error_result_not_a_crash() {
        let mut m = ScriptedModel::new(vec![
            vec![call("1", "frobnicate")],
            vec![call("2", "finish")],
        ]);
        let out = run(&mut m, "x".into(), TURNS).unwrap().text;
        assert_eq!(out, "x");
        assert!(m.seen[1][0].is_error);
        assert!(m.seen[1][0].content.contains("unknown tool"));
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
        // interleave a failing edit with views; failures aren't consecutive.
        let mut m = ScriptedModel::new(vec![
            vec![edit_call("1", "nope", "x")], // fail 1
            vec![call("2", "view")],           // resets the streak
            vec![edit_call("3", "nope", "x")], // fail 1 again
            vec![edit_call("4", "a", "b"), call("5", "finish")],
        ]);
        let out = run(&mut m, "a".into(), TURNS).unwrap().text;
        assert_eq!(out, "b");
    }

    #[test]
    fn hits_turn_limit() {
        // never finishes; only views.
        let turns = (0..30).map(|i| vec![call(&i.to_string(), "view")]).collect();
        let mut m = ScriptedModel::new(turns);
        let err = run(&mut m, "x".into(), 5).unwrap_err();
        assert!(err.to_string().contains("limit"));
    }
}
