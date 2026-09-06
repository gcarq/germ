/// Formats the given [`anyhow::Error`] including its causes
/// into a human readable string.
pub fn format_error(err: &anyhow::Error) -> String {
    let error_cause = err
        .chain()
        .skip(1)
        .enumerate()
        .map(|(i, cause)| format!("   {i}: {cause}"))
        .collect::<Vec<_>>()
        .join("\n");
    if error_cause.is_empty() {
        format!("{err}")
    } else {
        format!("{err}\nCaused by\n{error_cause}")
    }
}
