pub mod version;

use crate::ebuild::handler::prot::ParentMessage;
use crate::repository::Repository;
use anyhow::{Result, anyhow};
use log::{debug, error};
use std::process;

/// Checks if the given `word` is present anywhere in the list of `args`.
/// Returns `Err` if `word` contains whitespace or is not present.
pub fn contains_word(word: &str, args: &[String]) -> ParentMessage {
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
pub fn debug_print(args: &[String]) -> ParentMessage {
    debug!(target: "ebuild", "{}", args.join(" "));
    ParentMessage::Ok(None)
}

/// Logs the given `message` to `error!()` and exits with code 1 if `fatal` is true.
pub fn die(args: &[String], fatal: bool) -> ParentMessage {
    error!("die: {}", args.join(" "));
    if fatal {
        process::exit(1);
    }
    ParentMessage::Err(None)
}

/// Resolves the given eclass `name` from `repository` and returns the path as string.
pub fn resolve_eclass(name: &str, repository: &Repository) -> Result<ParentMessage> {
    let eclass = repository
        .eclasses
        .get(name)
        .ok_or_else(|| anyhow!("eclass {name} not found in {repository} or its masters"))?;

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
    use crate::ebuild::handler::prot::func::FuncCall;
    use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
    use crate::makenv::MakeEnv;
    use crate::package::cpv::CPV;
    use crate::package::version::PackageVersion;
    use std::path::PathBuf;

    #[test]
    fn test_exec_ebuild_fn_ok() {
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

        let cpv = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.2.3b", Some("alpha4"), None).unwrap(),
        )
        .unwrap();

        let ebuild = Ebuild {
            eapi: Eapi::Eight,
            cpv: &cpv,
            repo: &Repository::default(),
            path: PathBuf::default(),
        };
        let handler = EbuildPhaseHandler::new(&ebuild, EbuildPhase::Depend, &MakeEnv::default());
        for (func, response) in test_data {
            assert_eq!(handler.handle_request(func).unwrap(), response);
        }
    }
}
