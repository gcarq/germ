pub mod version;

use crate::ebuild::handler::prot::ParentMessage;
use crate::repository::Repository;
use anyhow::anyhow;
use log::{debug, warn};

/// Logs the given `args` using `debug!()`.
pub fn debug_print(args: &[String]) -> ParentMessage {
    debug!(target: "ebuild", "{}", args.join(" "));
    ParentMessage::Ok(None)
}

/// Logs the given `args` using `error!()`.
pub fn die(args: &[String], fatal: bool) -> ParentMessage {
    if fatal {
        ParentMessage::Die(args.join(" "))
    } else {
        warn!("die: {}", args.join(" "));
        ParentMessage::Err(None)
    }
}

/// Resolves the given eclass `name` from `repository` and returns the path as string.
pub fn resolve_eclass(name: &str, repository: &Repository) -> anyhow::Result<ParentMessage> {
    let eclass = repository
        .eclasses()?
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
    use crate::ebuild::handler::error::{ExecutionError, FuncCallError};
    use crate::ebuild::handler::prot::{FuncCall, FuncType};
    use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
    use crate::makenv::MakeEnv;
    use crate::package::cpv::CPV;
    use crate::package::version::PackageVersion;
    use crate::repository::test_support::{RepoSetFixture, RepositoryFixture};
    use std::path::PathBuf;

    fn with_handler(eapi: Eapi, test: impl FnOnce(&EbuildPhaseHandler)) {
        let fixture = RepoSetFixture::new(vec![RepositoryFixture::new("repo")]).unwrap();
        let cpv = CPV::new(
            "app-editors",
            "vim",
            PackageVersion::try_from("1.2.3b_alpha4").unwrap(),
        )
        .unwrap();
        let ebuild = Ebuild {
            eapi,
            cpv: &cpv,
            repo: fixture.get("repo").unwrap(),
            path: &PathBuf::default(),
        };
        let handler =
            EbuildPhaseHandler::new(&ebuild, EbuildPhase::Depend, &MakeEnv::default()).unwrap();
        test(&handler);
    }

    #[test]
    fn test_exec_ebuild_fn_ok() {
        // (func call, expected response)
        let test_data = [
            (
                FuncCall::from_raw("die", &["-n", "This is a non-fatal error"]).unwrap(),
                ParentMessage::Err(None),
            ),
            (
                FuncCall::from_raw("die", &["This is a fatal error"]).unwrap(),
                ParentMessage::Die("This is a fatal error".into()),
            ),
            (
                FuncCall::from_raw("has", &["foo", "foo", "bar", "baz"]).unwrap(),
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

        with_handler(Eapi::Eight, |handler| {
            for (func, response) in test_data {
                assert_eq!(handler.handle_request(func).unwrap(), response);
            }
        });
    }

    #[test]
    fn test_func_invalid_args() {
        with_handler(Eapi::Eight, |handler| {
            let args = vec!["invalid".to_owned(), "-".to_owned()];
            let error = handler
                .handle_request(FuncCall {
                    func: FuncType::VerRs,
                    args: args.clone(),
                })
                .unwrap_err();

            assert!(matches!(
                error,
                ExecutionError::FuncCall(FuncCallError::InvalidArgs {
                    func: FuncType::VerRs,
                    args: error_args,
                    ..
                }) if error_args == args
            ));
        });
    }

    #[test]
    fn test_func_unsupported() {
        with_handler(Eapi::Eight, |handler| {
            let error = handler
                .handle_request(FuncCall {
                    func: FuncType::HasV,
                    args: vec!["value".to_owned()],
                })
                .unwrap_err();

            assert!(matches!(
                error,
                ExecutionError::FuncCall(FuncCallError::Unsupported {
                    func: FuncType::HasV,
                    eapi: Eapi::Eight,
                })
            ));
        });
    }
}
