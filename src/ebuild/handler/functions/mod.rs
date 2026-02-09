use crate::conf::repos::ReposConf;
use crate::ebuild::handler::functions::version::{ver_cut, ver_rs, ver_test};
use crate::ebuild::handler::prot::{FuncType, Request, Response};
use crate::package::Package;
use anyhow::{Result, anyhow};
use std::process;

mod version;

/// Takes a `pkg`, `repos`and executes a ebuild function for the given `request`.
pub fn handle_request(pkg: &Package, repos: &ReposConf, request: &Request) -> Result<Response> {
    // At this point the package should have a resolved ebuild
    let eapi = &pkg.ebuild.as_ref().unwrap().eapi;
    match request.func {
        FuncType::ResolveEclass => match request.args {
            [name] => resolve_eclass(pkg, repos, name),
            _ => Err(anyhow!(
                "invalid arguments: __resolve_eclass <name>: {:?}",
                request.args
            )),
        },
        FuncType::ContainsWord => match request.args {
            [word, args @ ..] => Ok(contains_word(word, args)),
            _ => Err(anyhow!(
                "invalid arguments: contains_word <word> <string>: {:?}",
                request.args
            )),
        },
        FuncType::Die => match request.args {
            ["-n", args @ ..] => Ok(die(args, false)),
            args => Ok(die(args, true)),
        },
        FuncType::Has => match request.args {
            [word, args @ ..] => match args.contains(word) {
                true => Ok(Response::Ok(None)),
                false => Ok(Response::Err(None)),
            },
            _ => Err(anyhow!(
                "invalid arguments: has <word> <args>: {:?}",
                request.args
            )),
        },
        FuncType::HasV if eapi.is_hasv_supported() => match request.args {
            [word, args @ ..] => match args.contains(word) {
                true => Ok(Response::Ok(Some(word.to_string()))),
                false => Ok(Response::Err(None)),
            },
            _ => Err(anyhow!(
                "invalid arguments: hasv <word> <args>: {:?}",
                request.args
            )),
        },
        FuncType::HasQ if eapi.is_hasq_supported() => match request.args {
            [word, args @ ..] => match args.contains(word) {
                true => Ok(Response::Ok(None)),
                false => Ok(Response::Err(None)),
            },
            _ => Err(anyhow!(
                "invalid arguments: hasq <word> <args>: {:?}",
                request.args
            )),
        },
        FuncType::VerCut => match request.args {
            [range] => ver_cut(pkg, range, None),
            [range, version] => ver_cut(pkg, range, Some(version)),
            _ => Err(anyhow!(
                "invalid arguments: ver_cut <range> [<version>]: {:?}",
                request.args
            )),
        },
        FuncType::VerRs => ver_rs(pkg, request.args),
        FuncType::VerTest => match request.args {
            [op, v2] => ver_test(pkg, None, op, v2),
            [v1, op, v2] => ver_test(pkg, Some(v1), op, v2),
            _ => Err(anyhow!(
                "invalid arguments: ver_test [<v1>] op <v2>: {:?}",
                request.args
            )),
        },
        FuncType::HasV | FuncType::HasQ => Err(anyhow!(
            "unsupported function '{}' for EAPI '{eapi}'",
            request.func,
        )),
    }
}

/// Checks if the given `word` is present anywhere in the list of `args`.
/// Returns `Err` if `word` contains whitespace or is not present.
fn contains_word(word: &str, args: &[&str]) -> Response {
    if word.contains(' ') {
        return Response::Err(None);
    }
    match args
        .iter()
        .flat_map(|arg| arg.split_ascii_whitespace())
        .any(|w| w == word)
    {
        true => Response::Ok(None),
        false => Response::Err(None),
    }
}

/// Prints the given `message` to stderr and exits with code 1 if `fatal` is true.
/// Otherwise, it returns "1" as a string.
fn die(args: &[&str], fatal: bool) -> Response {
    eprintln!("die: {}", args.join(" "));
    if fatal {
        process::exit(1);
    }
    Response::Err(None)
}

/// Resolves the given eclass `name` and returns the path as string.
fn resolve_eclass(pkg: &Package, repos: &ReposConf, name: &str) -> Result<Response> {
    let eclass = match repos.get(&pkg.repository) {
        Some(repo) => match repo.eclasses.get(name) {
            Some(eclass) => eclass,
            // If the eclass is not in the same repository, we check the main repository
            None => match repos.main_repo().eclasses.get(name) {
                Some(eclass) => eclass,
                None => Err(anyhow!("eclass '{name}' not found"))?,
            },
        },
        None => Err(anyhow!(
            "repository '{}' assigned to package not found",
            pkg.repository
        ))?,
    };

    let path = eclass
        .path
        .to_str()
        .ok_or_else(|| anyhow!("eclass path contains invalid unicode"))?
        .to_owned();
    Ok(Response::Ok(Some(path)))
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
            "gentoo",
        )
        .unwrap()
        .with_ebuild(Ebuild {
            path: PathBuf::default(),
            eapi: Eapi::new("8").unwrap(),
        });

        // (request data, expected response)
        let test_data = [
            (
                vec!["FN", "die", "-n", "This is a non-fatal error"],
                Response::Err(None),
            ),
            (
                vec!["FN", "has", "foo", "foo", "bar", "baz"],
                Response::Ok(None),
            ),
            (
                vec![
                    "FN",
                    "contains_word",
                    "nodoc",
                    "buildpkg clean-logs fail-clean nodoc parallel-install split-log",
                ],
                Response::Ok(None),
            ),
            (
                vec!["FN", "ver_rs", "1-2", "-", "1.2.3.4"],
                Response::Ok(Some("1-2-3.4".into())),
            ),
            (
                vec!["FN", "ver_test", "6.0", "-gt", "5.0"],
                Response::Ok(None),
            ),
        ];
        for (data, response) in test_data {
            let request = Request::new(&data).unwrap();
            assert_eq!(
                handle_request(&pkg, &ReposConf::default(), &request).unwrap(),
                response
            )
        }
    }
}
