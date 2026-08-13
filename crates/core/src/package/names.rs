use crate::grammar::{CATEGORY, PACKAGE, REVISION, VERSION, VERSION_SUFFIXES};
use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::{fmt, str::FromStr, sync::LazyLock};

/// Regex for category name validation.
static CATEGORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A{CATEGORY}\z")).unwrap());

/// Regex for package name validation.
static PACKAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?!.*-(?:{VERSION})(?:{VERSION_SUFFIXES})(?:-r{REVISION})?\z){PACKAGE}\z"
    ))
    .unwrap()
});

/// Holds a validated category name.
#[derive(Archive, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CatName(Box<str>);

impl CatName {
    /// Creates a new [`CatName`] from the given `category`.
    pub fn new(category: &str) -> anyhow::Result<Self> {
        match CATEGORY_RE.is_match(category)? {
            true => Ok(Self(category.into())),
            false => bail!("invalid category name: '{category}'"),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CatName {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for CatName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Holds a validated package name.
#[derive(Archive, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PkgName(Box<str>);

impl PkgName {
    /// Creates a new [`PkgName`] from the given `package`.
    pub fn new(package: &str) -> anyhow::Result<Self> {
        match PACKAGE_RE.is_match(package)? {
            true => Ok(Self(package.into())),
            false => bail!("invalid package name: '{package}'"),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PkgName {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for PkgName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_name_ok() {
        let valid_names = ["foo", "foo-", "foo--", "foo+bar", "foo_+", "foo-r2"];
        for name in valid_names {
            let parsed = PkgName::new(name).unwrap();
            assert_eq!(parsed.as_str(), name);
        }
    }

    #[test]
    fn test_package_name_err() {
        let invalid_names = [
            "",
            "-foo",
            "+foo",
            "foo.bar",
            "foo/bar",
            "foo ",
            "foo*",
            "foo-1",
            "foo-1a",
            "foo-1_alpha",
            "foo-1_beta",
            "foo-1_pre",
            "foo-1_rc1",
            "foo-1_p2",
            "foo-1-r0",
            "foo-1-r2",
            "foo-r2-2",
        ];
        for name in invalid_names {
            assert!(PkgName::new(name).is_err(), "{name:?} should be invalid");
        }
    }
}
