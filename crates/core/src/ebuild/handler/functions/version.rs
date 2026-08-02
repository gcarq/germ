use crate::ebuild::handler::protocol::FunctionReply;
use crate::package::cpv::CPV;
use crate::package::version::PackageVersion;
use anyhow::bail;
use std::cmp::Ordering;

/// Implements the `ver_cut` function for ebuilds that extracts version components,
/// e.g.: `"ver_cut 1-2 1.2.3" -> "1.2"`.
///
/// Takes a `cpv`, a `range` string, and an optional `version` string as input.
/// The `range` specifies which components to extract from the version.
/// If `version` is `None`, the package's PV is used.
/// Returns `Err` if the EAPI does not support `ver_cut`.
pub fn ver_cut(cpv: &CPV, range: &str, version: Option<&str>) -> anyhow::Result<FunctionReply> {
    // Use PV as fallback if no version is provided
    let version = match version {
        Some(v) => v,
        None => &cpv.pv(),
    };
    let parts: Vec<(String, String)> = ver_split(version);
    let (mut start, end) = parse_range(range, parts.len())?;

    // Convert to flattened indices to be able to skip both separators and components
    if start > 0 {
        start = start * 2 - 1;
    }
    let end = end * 2;
    if start >= end {
        return Ok(FunctionReply::Ok(Some(String::new())));
    }
    let flat_components = parts
        .into_iter()
        .flat_map(|(sep, comp)| [sep, comp])
        .skip(start)
        .take(end - start)
        .collect::<String>();
    Ok(FunctionReply::Ok(Some(flat_components)))
}

/// Implements the `ver_rs` function for ebuilds that replaces separators for the given ranges,
/// e.g.: `"ver_rs 1-2 - 1.2.3.4" -> "1-2-3.4"`.
///
/// Takes a `cpv` and unsanitized function `args` as input.
/// If the len of `args` is odd, the last element is treated as the version string,
/// otherwise the package's PV is used.
/// Returns the modified version string or an `Err` if parsing fails.
pub fn ver_rs(cpv: &CPV, args: &[String]) -> anyhow::Result<FunctionReply> {
    if args.len() < 2 {
        bail!("ver_rs requires at least two arguments");
    }

    let (pairs, version) = match args.len() & 1 == 0 {
        true => (args, None),
        // Safe to unwrap, since we know args.len() must be > 0
        false => (&args[..args.len() - 1], Some(args.last().unwrap())),
    };

    // Fallback to PV if no version is provided
    let version = match version {
        Some(v) => v,
        None => &cpv.pv(),
    };

    let mut parts: Vec<(String, String)> = ver_split(version);
    let max_idx = parts.len().saturating_sub(1);

    let mut it = pairs.iter();
    while let (Some(range), Some(repl)) = (it.next(), it.next()) {
        let (start, end) = parse_range(range, max_idx)?;
        if start > end || start > max_idx {
            continue;
        }

        // Replace separators in the specified range
        for (i, (sep, _)) in parts.iter_mut().enumerate().take(end + 1).skip(start) {
            // Skip replacing the very first empty separator
            if i == 0 && sep.is_empty() {
                continue;
            }
            sep.clone_from(repl);
        }
    }
    let result = parts
        .into_iter()
        .fold(String::new(), |mut output, (sep, comp)| {
            output.push_str(&sep);
            output.push_str(&comp);
            output
        });
    Ok(FunctionReply::Ok(Some(result)))
}

/// Implements the `ver_test` function for ebuilds that checks if the relation
/// `version1 op version2` holds, e.g.: `"ver_test 6.0 -gt 5.0" -> true`.
///
/// Takes a `cpv`, an optional `version1`, an operator `op` and a `version2` as input.
/// If `version1` is `None`, the package's `PVR` is used.
/// The operator `op` must be one of: `"-gt"`, `"-ge"`, `"-eq"`, `"-ne"`, `"-le"` or `"-lt"`.
///
/// Returns `Ok(true)` if the comparison holds, `Ok(false)` otherwise.
pub fn ver_test(
    cpv: &CPV,
    version1: Option<&str>,
    op: &str,
    version2: &str,
) -> anyhow::Result<FunctionReply> {
    let v1 = match version1 {
        Some(v) => &PackageVersion::try_from(v)?,
        None => cpv.version(),
    };
    let v2 = &PackageVersion::try_from(version2)?;

    let does_match = match op {
        "-gt" => v1.cmp(v2) == Ordering::Greater,
        "-ge" => v1.cmp(v2) != Ordering::Less,
        "-eq" => v1.cmp(v2) == Ordering::Equal,
        "-ne" => v1.cmp(v2) != Ordering::Equal,
        "-le" => v1.cmp(v2) != Ordering::Greater,
        "-lt" => v1.cmp(v2) == Ordering::Less,
        _ => bail!("invalid operator: '{op}'"),
    };
    Ok(FunctionReply::from_bool(does_match))
}

/// Splits the given `version` into its components,
/// returning a vector of (separator, component) tuples.
/// Separators are non-alphanumeric characters that precede each component.
/// Components are sequences of either all digits or all letters.
fn ver_split(version: &str) -> Vec<(String, String)> {
    let mut chars = version.chars().peekable();
    let mut parts = Vec::new();

    while chars.peek().is_some() {
        // Check for non-alphanumeric separator
        let mut sep = String::new();
        if let Some(&c) = chars.peek()
            && !c.is_ascii_alphanumeric()
        {
            sep.push(c);
            chars.next();
        }

        // Check for component (either all digits or all letters)
        let mut comp = String::new();
        if let Some(&char) = chars.peek() {
            // Once we determine the type of the component,
            // we keep consuming chars of the same type
            let is_digit = char.is_ascii_digit();
            while let Some(&peeked_char) = chars.peek() {
                match is_digit {
                    true if !peeked_char.is_ascii_digit() => break,
                    false if !peeked_char.is_ascii_alphabetic() => break,
                    _ => {}
                }
                comp.push(peeked_char);
                chars.next();
            }
        }

        parts.push((sep, comp));
    }
    parts
}

/// Parses the given `range` of the form `A-B`, `A-`, or `N`.
/// Returns a tuple (start, end) representing the range,
/// where both start and end are inclusive and 1-based indices.
/// `max` is the maximum valid index (inclusive) for the range.
///
/// Returns `Err` if the range is invalid.
fn parse_range(range: &str, max: usize) -> anyhow::Result<(usize, usize)> {
    if let Some((a, b)) = range.split_once('-') {
        let start = match a.is_empty() {
            true => bail!("range must start with a number"),
            false => a.parse::<usize>()?,
        };
        let end = if b.is_empty() {
            max
        } else {
            let end = b.parse::<usize>()?;
            if start > end {
                bail!("range end must be >= start");
            }
            end
        };
        Ok((start, end.min(max)))
    } else {
        let num = range.parse::<usize>()?;
        Ok((num, num))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ver_cut_ok() {
        // (range, version, expected output)
        let test_cases = [
            ("1", None, "1"),
            ("1-1", None, "1"),
            ("1-2", None, "1.2"),
            ("2-", None, "2.3b_alpha4"),
            ("1-", None, "1.2.3b_alpha4"),
            ("3-4", None, "3b"),
            ("5", None, "alpha"),
            ("1-2", Some(".1.2.3"), "1.2"),
            ("0-2", Some(".1.2.3"), ".1.2"),
            ("2-3", Some("1.2.3."), "2.3"),
            ("2-", Some("1.2.3."), "2.3."),
            ("2-4", Some("1.2.3."), "2.3."),
            // Special cases (out of bounds, ...)
            ("0-2", Some("1.2.3"), "1.2"),
            ("2-5", Some("1.2.3"), "2.3"),
            ("4", Some("1.2.3"), ""),
            ("0", Some("1.2.3"), ""),
            ("4-", Some("1.2.3"), ""),
        ];
        let cpv = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.2.3b", Some("alpha4"), None).unwrap(),
        )
        .unwrap();
        for (range, version, expected) in test_cases {
            let response = FunctionReply::Ok(Some(expected.to_owned()));
            assert_eq!(
                ver_cut(&cpv, range, version).unwrap_or_else(|_| panic!(
                    "Failed for input range: {range}, version: {version:?}"
                )),
                response,
                "Failed for input range: {range}, version: {version:?}",
            );
        }
    }

    #[test]
    fn test_ver_cut_err() {
        let pkg = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.2.3b", Some("alpha4"), None).unwrap(),
        )
        .unwrap();
        let test_cases = ["-2", "3-2", "foo", "2-bar"];
        for range in test_cases {
            assert!(
                ver_cut(&pkg, range, None).is_err(),
                "Expected error for input range: {range}",
            );
        }
    }

    #[test]
    fn test_ver_rs_ok() {
        // (args, expected output)
        let test_cases = [
            (vec!["1", "-", "1.2.3"], "1-2.3"),
            (vec!["2", "-", "1.2.3"], "1.2-3"),
            (vec!["1-2", "-", "1.2.3.4"], "1-2-3.4"),
            (vec!["2-", "-", "1.2.3.4"], "1.2-3-4"),
            (vec!["2", ".", "1.2-3"], "1.2.3"),
            (vec!["3", ".", "1.2.3a"], "1.2.3.a"),
            (vec!["2-3", "-", "1.2_alpha4"], "1.2-alpha-4"),
            (vec!["3", "-", "2", "", "1.2.3b_alpha4"], "1.23-b_alpha4"),
            (vec!["3-5", "_", "4-6", "-", "a1b2c3d4e5"], "a1b_2-c-3-d4e5"),
            (vec!["1", "-", ".1.2.3"], ".1-2.3"),
            (vec!["0", "-", ".1.2.3"], "-1.2.3"),
            // Special cases (out of bounds, ...)
            (vec!["0", "-", "1.2.3"], "1.2.3"),
            (vec!["3", ".", "1.2.3"], "1.2.3"),
            (vec!["3-", ".", "1.2.3"], "1.2.3"),
            (vec!["3-5", ".", "1.2.3"], "1.2.3"),
            (vec!["2-3", "-"], "1.2-alpha-4"),
        ];
        let cpv = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.2", Some("alpha4"), None).unwrap(),
        )
        .unwrap();
        for (args, expected) in test_cases {
            let response = FunctionReply::Ok(Some(expected.to_owned()));
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert_eq!(
                ver_rs(&cpv, &args).unwrap_or_else(|_| panic!("Failed for input args: {args:?}")),
                response,
                "Failed for input args: {args:?}"
            );
        }
    }

    #[test]
    fn test_ver_rs_err() {
        let pkg = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.2b", Some("alpha4"), None).unwrap(),
        )
        .unwrap();
        let test_cases = [vec![], vec!["1"], vec!["foo", "-"]];
        for args in test_cases {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert!(
                ver_rs(&pkg, &args).is_err(),
                "Expected error for input args: {args:?}",
            );
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn test_ver_test_ok() {
        // (version1, op, version2, expected output)
        let test_cases = [
            (Some("6.0"), "-gt", "5.0", true),
            (Some("5.0"), "-gt", "5", true),
            (Some("1.0-r1"), "-gt", "1.0-r0", true),
            (
                Some("999999999999999999"),
                "-gt",
                "999999999999999998",
                true,
            ),
            (Some("1.0.0"), "-gt", "1.0", true),
            (Some("1.0.0"), "-gt", "1.0b", true),
            (Some("1b"), "-gt", "1", true),
            (Some("1b_p1"), "-gt", "1_p1", true),
            (Some("1.1b"), "-gt", "1.1", true),
            (Some("12.2.5"), "-gt", "12.2b", true),
            (Some("4.0"), "-lt", "5.0", true),
            (Some("5"), "-lt", "5.0", true),
            (Some("1.0_pre2"), "-lt", "1.0_p2", true),
            (Some("1.0_alpha2"), "-lt", "1.0_p2", true),
            (Some("1.0_alpha1"), "-lt", "1.0_beta1", true),
            (Some("1.0_beta3"), "-lt", "1.0_rc3", true),
            (
                Some("1.001000000000000001"),
                "-lt",
                "1.001000000000000002",
                true,
            ),
            (Some("1.00100000000"), "-lt", "1.001000000000000001", true),
            (
                Some("999999999999999998"),
                "-lt",
                "999999999999999999",
                true,
            ),
            (Some("1.01"), "-lt", "1.1", true),
            (Some("1.0-r0"), "-lt", "1.0-r1", true),
            (Some("1.0"), "-lt", "1.0-r1", true),
            (Some("1.0"), "-lt", "1.0.0", true),
            (Some("1.0b"), "-lt", "1.0.0", true),
            (Some("1_p1"), "-lt", "1b_p1", true),
            (Some("1"), "-lt", "1b", true),
            (Some("1.1"), "-lt", "1.1b", true),
            (Some("12.2b"), "-lt", "12.2.5", true),
            (Some("4.0"), "-eq", "4.0", true),
            (Some("1.0"), "-eq", "1.0", true),
            (Some("1.0-r0"), "-eq", "1.0", true),
            (Some("1.0"), "-eq", "1.0-r0", true),
            (Some("1.0-r0"), "-eq", "1.0-r0", true),
            (Some("1.0-r1"), "-eq", "1.0-r1", true),
            (Some("1"), "-eq", "2", false),
            (Some("1.0_alpha"), "-eq", "1.0_pre", false),
            (Some("1.0_beta"), "-eq", "1.0_alpha", false),
            (Some("1"), "-eq", "0.0", false),
            (Some("1.0-r0"), "-eq", "1.0-r1", false),
            (Some("1.0-r1"), "-eq", "1.0-r0", false),
            (Some("1.0"), "-eq", "1.0-r1", false),
            (Some("1.0-r1"), "-eq", "1.0", false),
            (Some("1.0"), "-eq", "1.0.0", false),
            (Some("1_p1"), "-eq", "1b_p1", false),
            (Some("1b"), "-eq", "1", false),
            (Some("1.1b"), "-eq", "1.1", false),
            (Some("12.2b"), "-eq", "12.2", false),
            (Some("1.0_alpha"), "-gt", "1_alpha", true),
            (Some("1.0_alpha"), "-gt", "1", true),
            (Some("1.0_alpha"), "-lt", "1.0", true),
            (Some("1.2.0.0_alpha7-r4"), "-gt", "1.2_alpha7-r4", true),
            (Some("0001"), "-eq", "1", true),
            (Some("01"), "-eq", "001", true),
            (Some("0001.1"), "-eq", "1.1", true),
            (Some("01.01"), "-eq", "1.01", true),
            (Some("1.010"), "-eq", "1.01", true),
            (Some("1.00"), "-eq", "1.0", true),
            (Some("1.0100"), "-eq", "1.010", true),
            (Some("1-r00"), "-eq", "1-r0", true),
            (Some("0_rc99"), "-lt", "0", true),
            (Some("011"), "-eq", "11", true),
            (Some("019"), "-eq", "19", true),
            (Some("1.2"), "-eq", "001.2", true),
            (Some("1.2"), "-gt", "1.02", true),
            (Some("1.2a"), "-lt", "1.2b", true),
            (Some("1.2_pre1"), "-gt", "1.2_pre1_beta2", true),
            (Some("1.2_pre1"), "-lt", "1.2_pre1_p2", true),
            (Some("1.00"), "-lt", "1.0.0", true),
            (Some("1.010"), "-eq", "1.01", true),
            (Some("1.01"), "-lt", "1.1", true),
            (Some("1.2_pre08-r09"), "-eq", "1.2_pre8-r9", true),
            (Some("0"), "-lt", "576460752303423488", true),
            (Some("0"), "-lt", "9223372036854775808", true),
            (None, "-eq", "1.0", true),
        ];

        let cpv = CPV::new(
            "sys-apps",
            "coreutils",
            PackageVersion::new("1.0", None, None).unwrap(),
        )
        .unwrap();
        for (version1, op, version2, expected) in test_cases {
            let response = FunctionReply::from_bool(expected);
            assert_eq!(
                ver_test(&cpv, version1, op, version2).unwrap_or_else(|_| panic!(
                    "Failed for input version1: {version1:?}, op: {op}, version2: {version2}"
                )),
                response,
                "Failed for input version1: {version1:?}, op: {op}, version2: {version2}"
            );
        }
    }

    #[test]
    fn test_ver_test_err() {
        let cpv = CPV::new(
            "sys-apps",
            "coreutils",
            PackageVersion::new("1.0", None, None).unwrap(),
        )
        .unwrap();
        let test_cases = [
            // Invalid argument order
            (Some("-lt"), "1", "2"),
            // Bad operators
            (Some("1"), "<", "2"),
            (Some("1"), "lt", "2"),
            (Some("1"), "-foo", "2"),
            // Malformed versions
            (Some(""), "-ne", "1"),
            (Some("1."), "-ne", "1"),
            (Some("1ab"), "-ne", "1"),
            (Some("b"), "-ne", "1"),
            (Some("1-r1_pre"), "-ne", "1"),
            (Some("1-pre1"), "-ne", "1"),
            (Some("1_foo"), "-ne", "1"),
            (Some("1_pre1.1"), "-ne", "1"),
            (Some("1-r1.0"), "-ne", "1"),
            (Some("cvs.9999"), "-ne", "9999"),
        ];
        for (version1, op, version2) in test_cases {
            assert!(
                ver_test(&cpv, version1, op, version2).is_err(),
                "Expected error for input version1: {version1:?}, op: {op}, version2: {version2}"
            );
        }
    }

    #[test]
    fn test_ver_split() {
        let test_cases = [
            ("1.2.3.", vec![("", "1"), (".", "2"), (".", "3"), (".", "")]),
            ("2.0.1", vec![("", "2"), (".", "0"), (".", "1")]),
            (
                "1.3b_alpha4",
                vec![("", "1"), (".", "3"), ("", "b"), ("_", "alpha"), ("", "4")],
            ),
        ];
        for (input, expected) in test_cases {
            let expected_tuples: Vec<(String, String)> = expected
                .into_iter()
                .map(|(s1, s2)| (s1.to_owned(), s2.to_owned()))
                .collect();
            assert_eq!(ver_split(input), expected_tuples);
        }
    }

    #[test]
    fn test_parse_range_ok() {
        let test_cases = [
            ("1-3", 5, (1, 3)),
            ("2-", 4, (2, 4)),
            ("2", 4, (2, 2)),
            ("1-7", 5, (1, 5)), // clamped to max
            ("5", 4, (5, 5)),   // a single number is not clamped
            ("5-", 4, (5, 4)),  // start is not clamped
        ];
        for (input, max, expected) in test_cases {
            assert_eq!(parse_range(input, max).unwrap(), expected);
        }
    }

    #[test]
    fn test_parse_range_err() {
        let test_cases = ["-3", "5-3", "foo", "2-bar"];
        for input in test_cases {
            assert!(
                parse_range(input, 3).is_err(),
                "Expected error for input: {input}",
            );
        }
    }
}
