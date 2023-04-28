pub fn diff(selected: &str, result: &str) -> String {
    diffy::create_patch(selected, result).to_string()
}

pub fn undiff(selected: &str, diff: &str) -> anyhow::Result<String> {
    let patch = diffy::Patch::from_str(diff)?;
    Ok(diffy::apply(selected, &patch)?)
}
