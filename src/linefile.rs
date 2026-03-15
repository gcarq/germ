use crate::types::FxHashSet;
use crate::utils::{FileFromPath, Inherit};
use anyhow::Result;

/// Represents a one-item-per-line file.
/// EAPI > 6 supports directories, in that case all files in that directory are merged together.
/// Lines beginning with a hyphen clear the content of previous lines that are equal to the
/// remainder of that line.
/// TODO: consider saving relevant file path and line numbers for better error messages.
#[derive(Default, Clone)]
#[cfg_attr(test, derive(Debug))]
pub struct LineBasedFile {
    lines: Vec<String>,
}

impl LineBasedFile {
    pub fn contains(&self, line: &str) -> bool {
        self.lines.iter().any(|l| l == line)
    }

    pub fn into_iter(self) -> impl Iterator<Item = String> {
        self.lines.into_iter()
    }

    pub fn into_inner(self) -> Vec<String> {
        self.lines
    }
}

impl<'a> FromIterator<&'a str> for LineBasedFile {
    /// Creates a new instance from the given line `iter`.
    /// Lines that are empty or start with `#` are ignored.
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let lines = iter
            .into_iter()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ToOwned::to_owned)
            .collect();
        Self { lines }
    }
}

impl FileFromPath for LineBasedFile {
    fn from_string(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(content.lines().collect::<Self>())
    }
}

impl Inherit for LineBasedFile {
    fn inherit_from(&mut self, parent: &LineBasedFile) {
        let mut parent_lines = parent.lines.clone();
        let mut seen = parent_lines.iter().cloned().collect::<FxHashSet<String>>();
        for line in &self.lines {
            if let Some(negated) = line.strip_prefix('-') {
                parent_lines.retain(|l| l != negated);
                seen.remove(negated);
            } else if seen.insert(line.clone()) {
                parent_lines.push(line.clone());
            }
        }
        self.lines = parent_lines;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_string() {
        let content = "
            # in a multilib profile we need multilib madness
            dev-libs/libffi abi_x86_32 abi_x86_64

            # new in 23.0
            app-arch/xz-utils abi_x86_32 abi_x86_64
            app-arch/zstd abi_x86_32 abi_x86_64
        ";

        let file = LineBasedFile::from_string(content.into()).unwrap();
        assert_eq!(
            file.lines,
            vec![
                "dev-libs/libffi abi_x86_32 abi_x86_64",
                "app-arch/xz-utils abi_x86_32 abi_x86_64",
                "app-arch/zstd abi_x86_32 abi_x86_64"
            ]
        );
        assert!(file.contains("dev-libs/libffi abi_x86_32 abi_x86_64"));
    }

    #[test]
    fn test_inherit_from() {
        let parent = LineBasedFile::from_string(
            "
            dev-libs/libffi
            app-arch/xz-utils
            app-arch/zstd
            "
            .into(),
        )
        .unwrap();
        let mut child = LineBasedFile::from_string(
            "
            -app-arch/xz-utils
            app-arch/zstd
            app-arch/rpm
            "
            .into(),
        )
        .unwrap();
        child.inherit_from(&parent);
        assert_eq!(
            child.lines,
            vec!["dev-libs/libffi", "app-arch/zstd", "app-arch/rpm"]
        );
    }
}
