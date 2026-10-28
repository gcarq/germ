use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use germ_pms::Atom;

use super::entry::{Entry, EntryValue, LineOrigin, Precedence};
use super::pkgfile::UseFlags;

/// Creates a test [`LineOrigin`] at `/dev/null` with the given `lineno`.
pub fn origin(lineno: u32) -> LineOrigin {
    LineOrigin::new(Arc::from(Path::new("/dev/null")), lineno)
}

/// Pairs each line with the `/dev/null` origin for its 1-based position.
pub fn with_origins<'a>(
    lines: impl IntoIterator<Item = &'a str>,
) -> impl Iterator<Item = (&'a str, LineOrigin)> {
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| (line, origin(index as u32 + 1)))
}

/// Creates a test [`Entry`] from `value` at `/dev/null` with the given `lineno`.
pub fn entry<T: EntryValue>(value: &str, prec: Precedence, lineno: u32) -> Entry<T> {
    Entry::new(value, prec, origin(lineno)).unwrap()
}

/// Returns the USE flags recorded for `atom`.
pub fn flags_for<'a>(records: &'a [(Atom, UseFlags)], atom: &str) -> anyhow::Result<&'a UseFlags> {
    let atom = Atom::new(atom)?;
    records
        .iter()
        .find_map(|(record, flags)| (record == &atom).then_some(flags))
        .ok_or_else(|| anyhow!("no USE flags for {atom}"))
}
