use crate::ebuild::Ebuild;
use crate::ebuild::handler::functions::version::{ver_cut, ver_rs, ver_test};
use crate::ebuild::handler::prot::ParentMessage;
use crate::ebuild::handler::prot::func::{FuncCall, FuncType};
use crate::repository::Repository;
use anyhow::{Result, anyhow};
use log::{debug, error};
use std::ops::Deref;
use std::process;

mod version;

/// Takes an `ebuild` and executes a function for the given [`FuncCall`].
///
/// Returns a [`ParentMessage`] with the result of the function or an `Err` if the request
/// is invalid or the function execution fails.
pub fn handle_request(ebuild: &Ebuild, call: FuncCall) -> Result<ParentMessage> {
    let args = call.args.deref();
    match call.func {
        FuncType::ResolveEclass => match args {
            [name] => resolve_eclass(name, ebuild.repo),
            _ => Err(anyhow!(
                "invalid arguments: __resolve_eclass <name>: {args:?}",
            )),
        },
        FuncType::ContainsWord => match args {
            [word, args @ ..] => Ok(contains_word(word, args)),
            _ => Err(anyhow!(
                "invalid arguments: contains_word <word> <string>: {args:?}",
            )),
        },
        FuncType::DebugPrint => Ok(debug_print(args)),
        FuncType::Die => match args {
            [first, args @ ..] if first == "-n" => Ok(die(args, false)),
            args => Ok(die(args, true)),
        },
        FuncType::Has => match args {
            [word, args @ ..] => match args.contains(word) {
                true => Ok(ParentMessage::Ok(None)),
                false => Ok(ParentMessage::Err(None)),
            },
            _ => Err(anyhow!("invalid arguments: has <word> <args>: {args:?}",)),
        },
        FuncType::HasV if ebuild.eapi.supports_hasv() => match args {
            [word, args @ ..] => match args.contains(word) {
                true => Ok(ParentMessage::Ok(Some(word.clone()))),
                false => Ok(ParentMessage::Err(None)),
            },
            _ => Err(anyhow!("invalid arguments: hasv <word> <args>: {args:?}",)),
        },
        FuncType::HasQ if ebuild.eapi.supports_hasq() => match args {
            [word, args @ ..] => match args.contains(word) {
                true => Ok(ParentMessage::Ok(None)),
                false => Ok(ParentMessage::Err(None)),
            },
            _ => Err(anyhow!("invalid arguments: hasq <word> <args>: {args:?}",)),
        },
        FuncType::VerCut => match args {
            [range] => ver_cut(ebuild.pkg, range, None),
            [range, version] => ver_cut(ebuild.pkg, range, Some(version)),
            _ => Err(anyhow!(
                "invalid arguments: ver_cut <range> [<version>]: {args:?}",
            )),
        },
        FuncType::VerRs => ver_rs(ebuild.pkg, args),
        FuncType::VerTest => match args {
            [op, v2] => ver_test(ebuild.pkg, None, op, v2),
            [v1, op, v2] => ver_test(ebuild.pkg, Some(v1), op, v2),
            _ => Err(anyhow!(
                "invalid arguments: ver_test [<v1>] op <v2>: {args:?}",
            )),
        },
        FuncType::HasV | FuncType::HasQ => Err(anyhow!(
            "unsupported function '{}' for EAPI '{}'",
            call.func,
            ebuild.eapi,
        )),
    }
}

/// Checks if the given `word` is present anywhere in the list of `args`.
/// Returns `Err` if `word` contains whitespace or is not present.
fn contains_word(word: &str, args: &[String]) -> ParentMessage {
    if word.contains(' ') {
        return ParentMessage::Err(None);
    }
    match args
        .iter()
        .flat_map(|arg| arg.split_ascii_whitespace())
        .any(|w| w == word)
    {
        true => ParentMessage::Ok(None),
        false => ParentMessage::Err(None),
    }
}

/// Logs the given `args` using `debug!()`.
fn debug_print(args: &[String]) -> ParentMessage {
    debug!(target: "ebuild", "{}", args.join(" "));
    ParentMessage::Ok(None)
}

/// Logs the given `message` to `error!()` and exits with code 1 if `fatal` is true.
fn die(args: &[String], fatal: bool) -> ParentMessage {
    error!("die: {}", args.join(" "));
    if fatal {
        process::exit(1);
    }
    ParentMessage::Err(None)
}

/// Resolves the given eclass `name` from `repository` and returns the path as string.
fn resolve_eclass(name: &str, repository: &Repository) -> Result<ParentMessage> {
    let eclass = repository
        .eclasses
        .get(name)
        .ok_or_else(|| anyhow!("{name} not found in {repository} or its masters"))?;

    let path = eclass
        .path
        .to_str()
        .ok_or_else(|| anyhow!("eclass path contains invalid unicode"))?
        .to_owned();
    Ok(ParentMessage::Ok(Some(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eapi::Eapi;
    use crate::ebuild::Ebuild;
    use crate::package::Package;
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
        .unwrap();
        let repo = Repository::default();

        let ebuild = Ebuild {
            path: PathBuf::default(),
            eapi: Eapi::Eight,
            pkg: &pkg,
            repo: &repo,
        };

        // (func call, expected response)
        let test_data = [
            (
                FuncCall::from_raw("die", &["-n", "This is a non-fatal error"]).unwrap(),
                ParentMessage::Err(None),
            ),
            (
                FuncCall::from_raw("has", &["foo", "foo", "bar", "baz"]).unwrap(),
                ParentMessage::Ok(None),
            ),
            (
                FuncCall::from_raw("contains_word", &["baz", "foo", "foobar", "baz"]).unwrap(),
                ParentMessage::Ok(None),
            ),
            (
                FuncCall::from_raw("ver_rs", &["1-2", "-", "1.2.3.4"]).unwrap(),
                ParentMessage::Ok(Some("1-2-3.4".into())),
            ),
            (
                FuncCall::from_raw("ver_test", &["6.0", "-gt", "5.0"]).unwrap(),
                ParentMessage::Ok(None),
            ),
        ];
        for (func, response) in test_data {
            assert_eq!(handle_request(&ebuild, func).unwrap(), response,)
        }
    }
}
