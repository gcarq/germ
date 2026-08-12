/// This module contains common regex patterns to validate and parse various
/// components as per the Portage Package Management Specification (PMS).
use constcat::concat;
use fancy_regex::Regex;
use std::sync::LazyLock;

/// PMS 3.1.1 category-name syntax, without a capture group.
pub const CATEGORY_COMPONENT: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_+.-]*";

/// Captures a PMS 3.1.1 category name.
pub const CATEGORY: &str = concat!(r"(?<category>", CATEGORY_COMPONENT, ")");

/// PMS 3.1.2 package-name syntax, without the package/version boundary check.
pub const PACKAGE_COMPONENT: &str = r"[a-zA-Z0-9_]([a-zA-Z0-9_+-]*[a-zA-Z0-9_+])?";

/// Captures the lexical package-name component of a CPV.
///
/// Use [`PKG_RE`] to validate a complete package name, including its boundary
/// with version syntax.
pub const PACKAGE: &str = concat!(r"(?<package>", PACKAGE_COMPONENT, ")");

/// PMS 3.1.3 slot-name syntax.
const SLOT_COMPONENT: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_+.-]*";

/// Captures a PMS 3.1.3 slot name.
pub const SLOT: &str = concat!("(", SLOT_COMPONENT, ")");

/// PMS 3.1.5 repository-name syntax.
const REPOSITORY_COMPONENT: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_-]*";

/// Captures a PMS 3.1.5 repository name.
pub const REPOSITORY: &str = concat!(r"(?<repo>", REPOSITORY_COMPONENT, ")");

const VERSION_NUMBER: &str = r"[0-9]+(?:\.[0-9]+)*[a-z]?";
const VERSION_SUFFIXES: &str = r"(?:_(?:alpha|beta|pre|rc|p)[0-9]*)*";
const REVISION_NUMBER: &str = r"[0-9]+";
const V_REV_COMPONENT: &str = concat!(
    VERSION_NUMBER,
    VERSION_SUFFIXES,
    "(?:-r",
    REVISION_NUMBER,
    ")?"
);

/// Captures a PMS version number and its suffixes.
pub const VERSION: &str = concat!(
    r"(?<version>",
    VERSION_NUMBER,
    r")(?<suffixes>",
    VERSION_SUFFIXES,
    ")"
);

/// Captures a PMS revision number.
pub const REVISION: &str = concat!(r"(?<revision>", REVISION_NUMBER, ")");

/// Captures a PMS version number, its suffixes, and an optional revision.
pub const V_REV: &str = concat!(VERSION, "(?:-r", REVISION, ")?");

/// Captures the package, version, suffixes, and optional revision in a CPV.
pub const PV_REV: &str = concat!(PACKAGE, "-", V_REV);

/// Regex to validate a repository name.
pub static REPO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(concat!(r"^", REPOSITORY, r"$")).unwrap());

/// Regex to validate a category name.
pub static CATEGORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(concat!(r"^", CATEGORY, r"$")).unwrap());

/// Regex to validate a complete package name.
pub static PKG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(r"^(?!.*-", V_REV_COMPONENT, r"$)", PACKAGE, r"$")).unwrap()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_regex_match() {
        let valid_names = ["gentoo", "my-repo_1", "repo123"];
        for name in valid_names {
            assert!(
                REPO_RE.is_match(name).unwrap(),
                "Repository '{name}' should be valid",
            );
        }
    }

    #[test]
    fn test_repository_regex_no_match() {
        let invalid_names = ["", "my repo", "repo!", "repo@123", "repo#name", "-repo"];
        for name in invalid_names {
            assert!(
                !REPO_RE.is_match(name).unwrap(),
                "Repository '{name}' should be invalid",
            );
        }
    }

    #[test]
    fn test_category_regex_match() {
        let valid_categories = vec![
            "app-editors",
            "dev-lang",
            "sys-apps",
            "media-libs",
            "net-misc",
            "net++",
            "foo.bar",
            "foo_bar",
            "foo+bar",
        ];
        for category in valid_categories {
            assert!(
                CATEGORY_RE.is_match(category).unwrap(),
                "Category '{category}' should be valid"
            );
        }
    }

    #[test]
    fn test_category_regex_no_match() {
        let invalid_categories = vec![
            "-invalid-category",
            ".hidden-category",
            "+plus-category",
            "invalid category",
        ];
        for category in invalid_categories {
            assert!(
                !CATEGORY_RE.is_match(category).unwrap(),
                "Category '{category}' should be invalid"
            );
        }
    }

    #[test]
    fn test_package_regex_match() {
        let valid_packages = vec![
            "vim",
            "rust",
            "python_",
            "memtest86+",
            "box64",
            "foo-bar",
            "foo-1-bar",
            "foo_bar",
            "foo+bar",
        ];
        for package in valid_packages {
            assert!(
                PKG_RE.is_match(package).unwrap(),
                "Package '{package}' should be valid"
            );
        }
    }

    #[test]
    fn test_package_regex_no_match() {
        let invalid_packages = vec![
            "",
            "invalid package",
            "memtest86-",
            "foo-1",
            "foo-1_alpha",
            "foo-1-r2",
            "foo-1-2",
            "bar!baz",
            "-bar",
            "+bar",
        ];
        for package in invalid_packages {
            assert!(
                !PKG_RE.is_match(package).unwrap(),
                "Package '{package}' should be invalid"
            );
        }
    }
}
