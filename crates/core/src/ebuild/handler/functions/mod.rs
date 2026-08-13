pub mod version;

use crate::ebuild::handler::error::{FuncCallError, PhaseExecutionError};
use crate::ebuild::handler::protocol::FunctionReply;
use crate::repository::Repository;
use anyhow::anyhow;
use log::{debug, warn};

/// Logs the given `args` using `debug!()`.
pub fn debug_print(args: &[String]) -> FunctionReply {
    debug!(target: "ebuild", "{}", args.join(" "));
    FunctionReply::Ok(None)
}

/// Logs the given `args` using `error!()`.
pub fn die(args: &[String], fatal: bool) -> FunctionReply {
    if fatal {
        FunctionReply::Die(args.join(" "))
    } else {
        warn!("die: {}", args.join(" "));
        FunctionReply::Err(None)
    }
}

/// Resolves the given eclass `name` from `repository` and returns the path as string.
pub fn resolve_eclass(
    name: &str,
    repository: &Repository,
) -> Result<FunctionReply, PhaseExecutionError> {
    let Some(eclass) = repository.eclasses.get(name) else {
        return Err(FuncCallError::EclassNotFound {
            name: name.to_owned(),
            repository: repository.to_string(),
        }
        .into());
    };

    let path = eclass
        .path
        .to_str()
        .ok_or_else(|| anyhow!("eclass path contains invalid unicode"))?
        .to_owned();
    Ok(FunctionReply::Ok(Some(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eapi::Eapi;
    use crate::ebuild::Ebuild;
    use crate::ebuild::handler::protocol::{FuncCall, FuncType};
    use crate::ebuild::handler::{EbuildPhase, EbuildPhaseHandler};
    use crate::makenv::MakeEnv;
    use crate::repository::test_support::{RepoBuilder, repo_set};
    use crate::test_support::cpv;
    use std::path::PathBuf;

    fn with_handler(eapi: Eapi, test: impl FnOnce(&EbuildPhaseHandler)) {
        let fixture = repo_set(vec![RepoBuilder::new("repo")]).unwrap();
        let cpv = cpv("app-editors", "vim", "1.2.3b_alpha4");
        let ebuild = Ebuild {
            eapi,
            cpv: &cpv,
            repo: fixture.get("repo").unwrap(),
            path: PathBuf::default(),
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
                FunctionReply::Err(None),
            ),
            (
                FuncCall::from_raw("die", &["This is a fatal error"]).unwrap(),
                FunctionReply::Die("This is a fatal error".into()),
            ),
            (
                FuncCall::from_raw("has", &["foo", "foo", "bar", "baz"]).unwrap(),
                FunctionReply::Ok(None),
            ),
            (
                FuncCall::from_raw("ver_rs", &["1-2", "-", "1.2.3.4"]).unwrap(),
                FunctionReply::Ok(Some("1-2-3.4".into())),
            ),
            (
                FuncCall::from_raw("ver_test", &["6.0", "-gt", "5.0"]).unwrap(),
                FunctionReply::Ok(None),
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
                PhaseExecutionError::FuncCall(FuncCallError::InvalidArgs {
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
                PhaseExecutionError::FuncCall(FuncCallError::Unsupported {
                    func: FuncType::HasV,
                    eapi: Eapi::Eight,
                })
            ));
        });
    }
}
