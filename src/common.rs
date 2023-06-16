use anyhow::anyhow as ah;
use similar::ChangeTag;

#[cfg(test)]
fn diff_inner(selected: &str, result: &str, algo: similar::Algorithm) -> String {
    use similar::TextDiff;

    let selected: Vec<&str> = selected.split('\n').collect();
    let result: Vec<&str> = result.split('\n').collect();

    let mut config = TextDiff::configure();
    config.algorithm(algo);

    let diff = config.diff_slices(&selected, &result);

    let mut output = String::new();
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                output += "delete ";
                output += change.value();
                output += "\n";
            }
            ChangeTag::Insert => {
                output += "insert ";
                output += change.value();
                output += "\n";
            }
            ChangeTag::Equal => {
                output += "goto ";
                output += change.value();
                output += "\n";
            }
        }
    }
    if output.ends_with('\n') {
        output.pop();
    }
    output
}

#[cfg(test)]
pub fn diff(selected: &str, result: &str) -> String {
    [
        similar::Algorithm::Myers,
        similar::Algorithm::Patience,
        // similar::Algorithm::Lcs, // Lcs algo is broken
    ]
    .iter()
    .map(|algo| diff_inner(selected, result, *algo))
    .min_by_key(|diff| diff.len())
    .unwrap()
}

fn get_changeset(diff: &'_ str) -> anyhow::Result<Vec<(ChangeTag, &'_ str)>> {
    let mut changeset = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("delete ") {
            changeset.push((ChangeTag::Delete, rest));
        } else if let Some(rest) = line.strip_prefix("insert ") {
            changeset.push((ChangeTag::Insert, rest));
        } else if let Some(rest) = line.strip_prefix("goto ") {
            changeset.push((ChangeTag::Equal, rest));
        } else if line.starts_with("note ") {
        } else {
            anyhow::bail!(
                "invalid diff format, every line should start with delete, insert, goto or note"
            );
        }
    }
    Ok(changeset)
}

pub fn undiff(selected: &str, diff: &str) -> anyhow::Result<String> {
    let changeset = get_changeset(diff)?;
    let mut output = Vec::<&str>::new();
    let mut selected = selected.split('\n');
    for (tag, line) in changeset {
        match tag {
            ChangeTag::Delete => loop {
                let ln = selected
                    .next()
                    .ok_or_else(|| ah!("selected text is shorter than the diff"))?;
                if line == ln {
                    break;
                }
                output.push(ln);
            },
            ChangeTag::Insert => {
                output.push(line);
            }
            ChangeTag::Equal => loop {
                let ln = selected
                    .next()
                    .ok_or_else(|| ah!("selected text is shorter than the diff"))?;
                output.push(ln);
                if line == ln {
                    break;
                }
            },
        }
    }
    output.append(&mut selected.collect());
    Ok(output.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: &[&str] = &[
        "hello",
        "hello\nworld",
        "hello\nworld\n",
        r#"
fn main() {
    println!("hello world");
}
"#,
        "
lorem ipsum dolor sit amet
consectetur adipiscing elit
sed do eiusmod tempor incididunt
ut labore et dolore magna aliqua
",
        "
lorem ipsum dolor sit amet
consectetur adipiscing elit
sed do eiusmod tempor incididunt
ut labore et dolore magna
",
        "
lorem ipsum dolor sit amet
sed do eiusmod tempor incididunt
ut labore et dolore magna aliqua
",
        "
lorem ipsum dolor sit amet
sed do eiusmod tempor incididunt
ut labore et dolore magna aliqua
",
        "
lorem ipsum dolor sit amet
consectetur adipiscing elit
sed do eiusmod tempor incididunt
sed do eiusmod tempor incididunt
ut labore et dolore magna aliqua
",
        "
lorem ipsum dolor sit amet
consectetur adipiscing elit
sed do eiusmod tempor incididunt
ut labore et dolore magna aliqua",
        "
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
   Compiling refac v0.1.2 (/home/bo-bert/refac)
    Finished test [unoptimized + debuginfo] target(s) in 1.63s
     Running unittests src/main.rs (target/debug/deps/refac-e4987e4890982537)

running 3 tests
test prompt::tests::test_sample_result ... ok
test prompt::tests::test_chat_prefix ... ok
test common::tests::diff_undiff ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
",
        "
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
   Compiling refac v0.1.4 (/home/bobert/refac)
    Finished test [unoptimized + debuginfo] target(s) in 1.63s

running 3 tests
test prompt::tests::test_sample_result ... ok
test prompt::tests::test_chat_prefix ... ok
=====sneaky line=====
test common::tests::diff_undiff ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
",
        "",
        "\n",
    ];

    #[test]
    fn diff_undiff() {
        for from in SAMPLES {
            for to in SAMPLES {
                println!("----------------------------------------");
                let diff = diff(from, to);
                println!("from:\n{}\n", from);
                println!("to:\n{}\n", to);
                println!("diff:\n{}\n", diff);
                let undiff = undiff(from, &diff).unwrap();
                println!("undiff:\n{}\n", undiff);
                assert_eq!(undiff, *to);
            }
        }
    }

    #[test]
    #[ignore]
    fn long_text_short_diff() {
        let from = include_str!("common.rs");
        let to = include_str!("common.rs");
        let diff = diff(from, to);
        assert_eq!(diff, "");
    }
}
