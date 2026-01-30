use crate::package::Package;
use anyhow::{Result, anyhow};

/// Handles the `ver_cut` function for ebuilds.
/// Takes a `pkg` and unsanitized function `args` as input.
/// Returns `Err` if the EAPI does not support `ver_cut`.
pub fn ver_cut(pkg: &Package, args: &[&str]) -> Result<String> {
    if let Some(ebuild) = &pkg.ebuild
        && !ebuild.eapi.has_ver_cut()
    {
        return Err(anyhow!("EAPI {} does not support ver_cut", ebuild.eapi));
    }

    // TODO: implement remaining functionality.
    assert_eq!(args.len(), 1, "TODO: ver_cut not implemented for {args:?}");
    let index = args[0]
        .parse::<usize>()?
        .checked_sub(1)
        .ok_or_else(|| anyhow!("ver_cut index must be greater than 0"))?;
    match pkg.version.pv_iter().nth(index) {
        Some(comp) => Ok(comp),
        None => Err(anyhow!(
            "index {index} out of range for version {}",
            pkg.version
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_ver_cut_valid() {
        let pkg = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("7.0.174z", Some("_alpha1"), None).unwrap(),
        )
        .unwrap();
        // index to expected output
        let test_cases = [
            (1, "7"),
            (2, "0"),
            (3, "174"),
            (4, "z"),
            (5, "alpha"),
            (6, "1"),
        ];
        for (index, expected) in test_cases {
            assert_eq!(ver_cut(&pkg, &[&index.to_string()]).unwrap(), expected);
        }
    }
}
