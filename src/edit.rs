//! Structured edits and how they're applied.
//!
//! The model calls a single-edit `edit` tool, possibly several times in one turn
//! (both providers support parallel tool calls); refac applies each `{old, new}`
//! replacement to the selected text. The hard part is that the model's `old`
//! rarely matches byte-for-byte — indentation drifts, whitespace reflows, a
//! block gets reworded. So matching runs a chain of progressively looser
//! strategies, exact first, and the first candidate that lands a *unique* hit
//! wins. A match that's missing or ambiguous is an error fed back to the model,
//! never a silent mis-apply: a wrong edit is worse than a refused one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// `schemars` turns the field doc comments below into the model-facing JSON-schema
// descriptions, so they're verbatim model instructions, not narration for readers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct Edit {
    /// exact text to replace
    pub old: String,
    /// replacement text
    pub new: String,
    /// replace every occurrence
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    NotFound { old: String },
    Ambiguous { old: String, count: usize },
    NoChange { old: String },
    EmptyOld,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::NotFound { old } => write!(
                f,
                "could not find this text to edit (copy it verbatim from the selection): {old:?}"
            ),
            EditError::Ambiguous { old, count } => write!(
                f,
                "found {count} matches for {old:?}; add surrounding context to make it unique, or set replace_all"
            ),
            EditError::NoChange { old } => {
                write!(f, "old and new are identical, so this edit does nothing: {old:?}")
            }
            EditError::EmptyOld => write!(
                f,
                "old is empty; to insert, anchor on existing text and include it in both old and new"
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// Walks the replacer chain (exact first) and requires a unique hit unless
/// `replace_all`. Folded over a turn's edits, so a later edit sees what an
/// earlier one produced.
pub fn apply(src: &str, edit: &Edit) -> Result<String, EditError> {
    if edit.old.is_empty() {
        return Err(EditError::EmptyOld);
    }
    if edit.old == edit.new {
        return Err(EditError::NoChange {
            old: edit.old.clone(),
        });
    }

    // Track the best diagnosis across the chain: an ambiguous candidate is a
    // more useful complaint than "not found", so remember it if nothing unique
    // turns up.
    let mut ambiguous: Option<usize> = None;

    for replacer in CHAIN {
        for cand in replacer(src, &edit.old) {
            // A blank `old` line trims to "" and yields an empty span; matching
            // "" hits between every char, so `replace_all` would splatter `new`
            // across the whole buffer. Skip it — an empty candidate is never a
            // real match.
            if cand.is_empty() {
                continue;
            }
            let count = src.matches(cand.as_str()).count();
            match (count, edit.replace_all) {
                (0, _) => continue,
                (_, true) => return Ok(src.replace(cand.as_str(), &edit.new)),
                (1, false) => {
                    let i = src.find(cand.as_str()).expect("count == 1");
                    let mut out = String::with_capacity(src.len() - cand.len() + edit.new.len());
                    out.push_str(&src[..i]);
                    out.push_str(&edit.new);
                    out.push_str(&src[i + cand.len()..]);
                    return Ok(out);
                }
                (n, false) => ambiguous = Some(ambiguous.map_or(n, |m| m.max(n))),
            }
        }
    }

    Err(match ambiguous {
        Some(count) => EditError::Ambiguous {
            old: edit.old.clone(),
            count,
        },
        None => EditError::NotFound {
            old: edit.old.clone(),
        },
    })
}

/// A replacer yields candidate substrings of `src` to look for, fuzzy intent but
/// the yielded string is always exact text *from* `src` (or `old` itself, for
/// the exact replacer) so the caller can find it and check uniqueness uniformly.
type Replacer = fn(src: &str, old: &str) -> Vec<String>;

/// Exact first, then progressively looser. Order matters: a precise match must
/// win before a fuzzy one gets a chance.
const CHAIN: &[Replacer] = &[
    simple,
    line_trimmed,
    block_anchor,
    whitespace_normalized,
    indentation_flexible,
];

fn simple(_src: &str, old: &str) -> Vec<String> {
    vec![old.to_string()]
}

fn lines_with_offsets(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for line in s.split_inclusive('\n') {
        out.push((start, line.strip_suffix('\n').unwrap_or(line)));
        start += line.len();
    }
    out
}

fn span(src: &str, lines: &[(usize, &str)], i: usize, k: usize) -> String {
    let start = lines[i].0;
    let end = lines[k].0 + lines[k].1.len();
    src[start..end].to_string()
}

/// Match line-by-line ignoring each line's surrounding whitespace; yield the
/// original (untrimmed) source span so indentation is preserved on splice.
fn line_trimmed(src: &str, old: &str) -> Vec<String> {
    let src_lines = lines_with_offsets(src);
    let old_lines: Vec<&str> = lines_with_offsets(old).iter().map(|(_, l)| *l).collect();
    let n = old_lines.len();
    if n == 0 || n > src_lines.len() {
        return vec![];
    }
    let mut out = Vec::new();
    for i in 0..=src_lines.len() - n {
        if (0..n).all(|j| src_lines[i + j].1.trim() == old_lines[j].trim()) {
            out.push(span(src, &src_lines, i, i + n - 1));
        }
    }
    out
}

/// For 3+ line blocks: anchor on the first and last (trimmed) lines, and accept
/// the window only if a majority of its non-empty middle lines also match. Lets
/// a reworded interior through while resisting wild matches.
fn block_anchor(src: &str, old: &str) -> Vec<String> {
    let src_lines = lines_with_offsets(src);
    let old_lines: Vec<&str> = lines_with_offsets(old).iter().map(|(_, l)| *l).collect();
    let n = old_lines.len();
    if n < 3 || n > src_lines.len() {
        return vec![];
    }
    let first = old_lines[0].trim();
    let last = old_lines[n - 1].trim();
    let mut out = Vec::new();
    for i in 0..=src_lines.len() - n {
        if src_lines[i].1.trim() != first || src_lines[i + n - 1].1.trim() != last {
            continue;
        }
        let mut considered = 0;
        let mut matched = 0;
        for j in 1..n - 1 {
            let o = old_lines[j].trim();
            if o.is_empty() {
                continue;
            }
            considered += 1;
            if src_lines[i + j].1.trim() == o {
                matched += 1;
            }
        }
        // Require some non-empty middle line to actually match — anchors alone
        // (an all-blank middle) are too weak to trust.
        if considered > 0 && matched * 2 >= considered {
            out.push(span(src, &src_lines, i, i + n - 1));
        }
    }
    out
}

/// Collapse `old` to whitespace-insensitive tokens and find a source region
/// holding those tokens in order, separated only by whitespace.
fn whitespace_normalized(src: &str, old: &str) -> Vec<String> {
    let tokens: Vec<&str> = old.split_whitespace().collect();
    if tokens.is_empty() {
        return vec![];
    }
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = src[from..].find(tokens[0]) {
        let start = from + rel;
        // Advance past the first char of this match (not one byte) so the next
        // search stays on a char boundary even for multi-byte text.
        from = start + src[start..].chars().next().map_or(1, char::len_utf8);
        let mut pos = start + tokens[0].len();
        let mut ok = true;
        for tok in &tokens[1..] {
            let mut p = pos;
            while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            if p == pos || !src[p..].starts_with(tok) {
                ok = false;
                break;
            }
            pos = p + tok.len();
        }
        if ok {
            out.push(src[start..pos].to_string());
        }
    }
    out
}

/// Strip common leading indentation from `old` and from each same-height source
/// window; where the dedented forms match, yield the original window.
fn indentation_flexible(src: &str, old: &str) -> Vec<String> {
    let src_lines = lines_with_offsets(src);
    let old_lines: Vec<&str> = lines_with_offsets(old).iter().map(|(_, l)| *l).collect();
    let n = old_lines.len();
    if n == 0 || n > src_lines.len() {
        return vec![];
    }
    let old_dedent = dedent(&old_lines);
    let mut out = Vec::new();
    for i in 0..=src_lines.len() - n {
        let window: Vec<&str> = (0..n).map(|j| src_lines[i + j].1).collect();
        if dedent(&window) == old_dedent {
            out.push(span(src, &src_lines, i, i + n - 1));
        }
    }
    out
}

fn dedent(lines: &[&str]) -> Vec<String> {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    // `indent` is the min byte-width across lines, so on a given line it can land
    // mid-char (multi-byte leading whitespace) — `get` declines that, no panic.
    lines
        .iter()
        .map(|l| l.get(indent..).unwrap_or(l).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> Edit {
        Edit {
            old: old.into(),
            new: new.into(),
            replace_all: false,
        }
    }

    fn run(text: &str, old: &str, new: &str) -> Result<String, EditError> {
        apply(text, &edit(old, new))
    }

    /// Fold `apply` over a turn's worth of edits, as the driver does.
    fn apply_seq(text: &str, edits: &[Edit]) -> Result<String, EditError> {
        let mut buf = text.to_string();
        for e in edits {
            buf = apply(&buf, e)?;
        }
        Ok(buf)
    }

    #[test]
    fn exact_substring() {
        assert_eq!(
            run("Me like toast.", "Me like", "I like").unwrap(),
            "I like toast."
        );
    }

    #[test]
    fn batch_applies_in_order() {
        // a later edit can target text an earlier edit produced.
        let edits = vec![edit("foo", "bar"), edit("bar", "baz")];
        assert_eq!(apply_seq("foo", &edits).unwrap(), "baz");
        let edits = vec![edit("one", "1"), edit("two", "2")];
        assert_eq!(apply_seq("one two", &edits).unwrap(), "1 2");
    }

    #[test]
    fn insertion_via_anchor() {
        let got = run(
            "def add(a, b):\n    return a + b\n",
            "def add(a, b):",
            "def add(a, b):\n    \"\"\"Sum.\"\"\"",
        )
        .unwrap();
        assert_eq!(
            got,
            "def add(a, b):\n    \"\"\"Sum.\"\"\"\n    return a + b\n"
        );
    }

    #[test]
    fn deletion_via_empty_new() {
        assert_eq!(
            run("hello cruel world", " cruel", "").unwrap(),
            "hello world"
        );
    }

    #[test]
    fn ambiguous_without_replace_all() {
        assert!(matches!(
            run("x x x", "x", "y"),
            Err(EditError::Ambiguous { count: 3, .. })
        ));
    }

    #[test]
    fn replace_all_when_requested() {
        let e = Edit {
            old: "x".into(),
            new: "y".into(),
            replace_all: true,
        };
        assert_eq!(apply_seq("x x x", &[e]).unwrap(), "y y y");
    }

    #[test]
    fn not_found_is_reported() {
        assert!(matches!(
            run("hello", "goodbye", "hi"),
            Err(EditError::NotFound { .. })
        ));
    }

    #[test]
    fn empty_old_rejected() {
        assert!(matches!(run("hello", "", "x"), Err(EditError::EmptyOld)));
    }

    #[test]
    fn noop_rejected() {
        assert!(matches!(
            run("hello", "hello", "hello"),
            Err(EditError::NoChange { .. })
        ));
    }

    #[test]
    fn line_trimmed_tolerates_indent_drift() {
        let src = "fn main() {\n        let x = 1;\n}\n";
        let got = run(src, "let x = 1;", "let x = 2;").unwrap();
        assert_eq!(got, "fn main() {\n        let x = 2;\n}\n");
    }

    #[test]
    fn dedented_old_matches_indented_source() {
        // The model wrote `old` without the source's indentation; we still find
        // the block. `new` is spliced verbatim, so the model owns the
        // indentation it wants in the result.
        let src = "if cond:\n        a = 1\n        b = 2\n";
        let old = "a = 1\nb = 2";
        let new = "        a = 10\n        b = 20";
        let got = run(src, old, new).unwrap();
        assert_eq!(got, "if cond:\n        a = 10\n        b = 20\n");
    }

    #[test]
    fn whitespace_normalized_reflow() {
        let got = run("foo    +    bar", "foo + bar", "baz").unwrap();
        assert_eq!(got, "baz");
    }

    #[test]
    fn whitespace_normalized_multibyte_no_panic() {
        // Regression: a non-ASCII first token must not slice mid-char.
        assert!(matches!(
            run("α   β", "α x", "z"),
            Err(EditError::NotFound { .. })
        ));
        assert_eq!(run("α    +    β", "α + β", "z").unwrap(), "z");
    }

    #[test]
    fn block_anchor_reworded_middle() {
        let src = "fn f() {\n    let a = compute();\n    let b = a + 1;\n    return b;\n}";
        // The model's `old` got the last middle line wrong (return b -> return
        // result). Exact and line-trimmed both miss; the first/last anchors plus
        // a majority of matching middle lines pin the real region.
        let old = "fn f() {\n    let a = compute();\n    let b = a + 1;\n    return result;\n}";
        let got = run(src, old, "fn f() { 42 }").unwrap();
        assert_eq!(got, "fn f() { 42 }");
    }

    #[test]
    fn blank_old_does_not_splatter_under_replace_all() {
        // A whitespace-only `old` trims to "" and the line matchers yield empty
        // spans; without the empty-candidate guard, replace_all on "" would
        // rewrite between every char. It must report NotFound instead.
        let e = Edit {
            old: " ".into(),
            new: "X".into(),
            replace_all: true,
        };
        assert!(matches!(
            apply("a\n\nb", &e),
            Err(EditError::NotFound { .. })
        ));
    }

    #[test]
    fn exact_beats_fuzzy_for_uniqueness() {
        // two indentation-equal blocks, but an exact match is unique → applied.
        let src = "  a = 1\n    a = 1\n";
        let got = run(src, "    a = 1", "    a = 2").unwrap();
        assert_eq!(got, "  a = 1\n    a = 2\n");
    }
}
