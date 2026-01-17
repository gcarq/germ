use crate::utils::{FileFromPath, Inherit};
use anyhow::Result;

/// Represents a one-item-per-line file.
/// EAPI > 6 supports directories, in that case all files in that directory are merged together.
/// Lines beginning with a hyphen clear the content of previous lines that are equal to the
/// remainder of that line.
/// TODO: consider saving relevant file path and line numbers for better error messages.
#[derive(Debug, Default, Clone)]
pub struct LineBasedFile {
    lines: Vec<String>,
}

impl LineBasedFile {
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.lines.iter()
    }

    pub fn into_iter(self) -> impl Iterator<Item = String> {
        self.lines.into_iter()
    }

    pub fn contains(&self, line: &str) -> bool {
        self.iter().any(|l| l == line)
    }
}

impl FileFromPath for LineBasedFile {
    fn from_file_content(content: String) -> Result<Self>
    where
        Self: Sized,
    {
        let lines = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.to_string())
            .collect();
        Ok(Self { lines })
    }
}

impl Inherit for LineBasedFile {
    fn inherit_from(&mut self, parent: &LineBasedFile) {
        let mut lines = parent.lines.clone();
        for line in &self.lines {
            if line.starts_with('-') {
                lines.retain(|l| l != &line[1..]);
            } else if !lines.contains(line) {
                lines.push(line.clone());
            }
        }
        self.lines = lines;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_file_content() {
        let content = "
            # in a multilib profile we need multilib madness
            dev-libs/libffi abi_x86_32 abi_x86_64

            # new in 23.0
            app-arch/xz-utils abi_x86_32 abi_x86_64
            app-arch/zstd abi_x86_32 abi_x86_64
        ";

        let file = LineBasedFile::from_file_content(content.to_string()).unwrap();
        assert_eq!(
            file.lines,
            vec![
                "dev-libs/libffi abi_x86_32 abi_x86_64",
                "app-arch/xz-utils abi_x86_32 abi_x86_64",
                "app-arch/zstd abi_x86_32 abi_x86_64"
            ]
        );
    }

    #[test]
    fn test_inherit_from() {
        let parent = LineBasedFile::from_file_content(
            "
            dev-libs/libffi
            app-arch/xz-utils
            app-arch/zstd
            "
            .into(),
        )
        .unwrap();
        let mut child = LineBasedFile::from_file_content(
            "
            -app-arch/xz-utils
            app-arch/zstd
            "
            .into(),
        )
        .unwrap();
        child.inherit_from(&parent);
        assert_eq!(child.lines, vec!["dev-libs/libffi", "app-arch/zstd"]);
    }
}
