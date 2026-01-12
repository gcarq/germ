use crate::profile::InheritFrom;
use crate::utils::FileFromPath;
use anyhow::Result;

/// Represents a one-item-per-line file.
/// EAPI > 6 supports directories, in that case all files in that directory are merged together.
/// Lines beginning with a hyphen clear the content of previous lines that are equal to the
/// remainder of that line.
#[derive(Debug, Default, Clone)]
pub struct LineBasedFile {
    lines: Vec<String>,
}

impl LineBasedFile {
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.lines.iter()
    }

    pub fn contains(&self, line: &str) -> bool {
        self.iter().any(|l| l == line)
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.lines.clone()
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

impl InheritFrom for LineBasedFile {
    fn inherit_from(&mut self, parent: &LineBasedFile) {
        let mut lines = parent.lines.clone();
        for line in &self.lines {
            if line.starts_with('-') {
                lines.retain(|l| l != &line[1..]);
            }
            lines.push(line.clone());
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
}
