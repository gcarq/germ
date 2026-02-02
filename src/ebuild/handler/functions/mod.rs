use crate::ebuild::handler::functions::version::{ver_cut, ver_rs, ver_test};
use crate::package::Package;
use anyhow::{Result, anyhow};

mod version;

/// Takes a `pkg` and executes a ebuild function with the given `name` and `args`.
pub fn exec_ebuild_fn(pkg: &Package, name: &str, args: &[&str]) -> Result<String> {
    // At this point the package should have a resolved ebuild
    // TODO: Improve ergonomics of accessing ebuild
    let eapi = &pkg.ebuild.as_ref().unwrap().eapi;
    match name {
        "ver_cut" if eapi.has_ver_cut() => match args {
            [range] => ver_cut(pkg, range, None),
            [range, version] => ver_cut(pkg, range, Some(version)),
            _ => Err(anyhow!("invalid arguments: ver_cut <range> [<version>]")),
        },
        "ver_rs" if eapi.has_ver_rs() => ver_rs(pkg, args),
        "ver_test" if eapi.has_ver_test() => {
            let result = match args {
                [operator, version2] => ver_test(pkg, None, operator, version2)?,
                [version1, operator, version2] => {
                    ver_test(pkg, Some(version1), operator, version2)?
                }
                _ => Err(anyhow!("invalid arguments: ver_test [<v1>] op <v2>"))?,
            };
            Ok(bool_to_bash(result))
        }
        _ => Err(anyhow!(
            "'{name} {}' cannot be executed.\n\
            Either this function is not available for EAPI {eapi} or hasn't been implemented yet.",
            args.join(" ")
        )),
    }
}

/// Converts the given `value` to a boolean value for bash
fn bool_to_bash(value: bool) -> String {
    match value {
        true => "0".into(),
        false => "1".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eapi::Eapi;
    use crate::ebuild::Ebuild;
    use crate::package::version::PackageVersion;
    use std::path::PathBuf;

    #[test]
    fn test_exec_ebuild_fn_ok() {
        let pkg = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.2.3b", Some("alpha4"), None).unwrap(),
        )
        .unwrap()
        .with_ebuild(Ebuild {
            path: PathBuf::default(),
            eapi: Eapi::new("8").unwrap(),
        });

        // (fn_name, args, expected result)
        let test_data = [
            ("ver_cut", vec!["1-2", "1.2.3"], "1.2"),
            ("ver_rs", vec!["1-2", "-", "1.2.3.4"], "1-2-3.4"),
            ("ver_test", vec!["6.0", "-gt", "5.0"], "0"),
        ];
        for (fn_name, args, expected) in test_data {
            assert_eq!(exec_ebuild_fn(&pkg, fn_name, &args).unwrap(), expected)
        }
    }
}
