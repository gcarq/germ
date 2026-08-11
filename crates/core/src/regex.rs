/// This module contains common regex patterns to validate and parse various
/// components as per the Portage Package Management Specification (PMS).
use constcat::concat;
use regex::Regex;
use std::sync::LazyLock;

/// PMS 3.1.1 Category names
/// A category name may contain any of the characters [A-Za-z0-9+_.-].
/// It must not begin with a hyphen, a dot or a plus sign.
pub const CATEGORY: &str = r"(?<category>[a-zA-Z0-9_][a-zA-Z0-9_+.-]*)";

/// PMS 3.1.2 Package names
/// A package name may contain any of the characters [A-Za-z0-9+_-].
/// It must not begin with a hyphen or a plus sign, and must not end in a hyphen
/// followed by anything matching the version syntax
pub const PACKAGE: &str = r"(?<package>[a-zA-Z0-9_]([a-zA-Z0-9_+-]*[a-zA-Z0-9_+])?)";

/// PMS 3.1.3 Slot names
/// A slot name may contain any of the characters [A-Za-z0-9+_.-].
/// It must not begin with a hyphen, a dot or a plus sign.
pub const SLOT: &str = r"([a-zA-Z0-9_][a-zA-Z0-9_+.-]*)";

/// Regex to validate repository names according to PMS 3.1.5.
pub const REPOSITORY: &str = r"(?P<repo>[a-zA-Z0-9_][a-zA-Z0-9_-]*)";

pub const VERSION: &str =
    r"(?<version>[0-9]+(?:\.[0-9]+)*[a-z]?)(?<suffixes>(?:_(?:alpha|beta|pre|rc|p)[0-9]*)*)";

pub const REVISION: &str = r"(?<revision>[0-9]+)";

pub const V_REV: &str = concat!(VERSION, "(?:-r", REVISION, ")?");

pub const PV_REV: &str = concat!(PACKAGE, "-", V_REV);

/// Regex to validate a repository names.
pub static REPO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{REPOSITORY}$")).unwrap());

/// Regex to validate a category name.
pub static CATEGORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{CATEGORY}$")).unwrap());

/// Regex to validate a package name.
pub static PKG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{PACKAGE}$")).unwrap());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_regex_match() {
        let valid_names = ["gentoo", "my-repo_1", "repo123"];
        for name in valid_names {
            assert!(
                REPO_RE.is_match(name),
                "Repository '{name}' should be valid",
            );
        }
    }

    #[test]
    fn test_repository_regex_no_match() {
        let invalid_names = ["", "my repo", "repo!", "repo@123", "repo#name", "-repo"];
        for name in invalid_names {
            assert!(
                !REPO_RE.is_match(name),
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
                CATEGORY_RE.is_match(category),
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
                !CATEGORY_RE.is_match(category),
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
            "foo_bar",
            "foo+bar",
        ];
        for package in valid_packages {
            assert!(
                PKG_RE.is_match(package),
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
            "bar!baz",
            "-bar",
            "+bar",
        ];
        for package in invalid_packages {
            assert!(
                !PKG_RE.is_match(package),
                "Package '{package}' should be invalid"
            );
        }
    }
}
