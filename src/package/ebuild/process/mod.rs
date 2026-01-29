mod ipc;

use crate::consts::BASH_BINARY_PATH;
use crate::package::Package;
use crate::package::ebuild::Ebuild;
use crate::package::ebuild::process::ipc::IpcHandler;
use anyhow::{Context, Result, anyhow};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, pipe};
use std::collections::HashMap;
use std::ffi::CString;
use std::mem;
use std::os::fd::AsRawFd;
use std::str::FromStr;

pub enum EbuildPhase {
    Metadata,
}

pub struct EbuildProcess<'a> {
    package: &'a Package,
    phase: EbuildPhase,
    pid: Pid,
    ipc: IpcHandler,
}

impl<'a> EbuildProcess<'a> {
    pub fn new(package: &'a Package, phase: EbuildPhase) -> Result<Self> {
        // IPC pipe for parent -> child
        let (child_reader, parent_writer) = pipe().with_context(|| "unable to create pipe")?;
        // IPC pipe for child -> parent
        let (parent_reader, child_writer) = pipe().with_context(|| "unable to create pipe")?;
        let parent_writer = mem::ManuallyDrop::new(parent_writer);
        let parent_reader = mem::ManuallyDrop::new(parent_reader);

        let mut actions = PosixSpawnFileActions::init()
            .with_context(|| "unable to init posix spawn file actions")?;
        let attrs =
            PosixSpawnAttr::init().with_context(|| "unable to init posix spawn attributes")?;

        actions.add_dup2(child_reader.as_raw_fd(), 3)?;
        actions.add_dup2(child_writer.as_raw_fd(), 6)?;
        actions.add_close(parent_writer.as_raw_fd())?;
        actions.add_close(parent_reader.as_raw_fd())?;

        if package.ebuild.is_none() {
            return Err(anyhow!("no ebuild found for package: {package}"));
        }

        let env = Self::create_ebuild_env(package)?
            .iter()
            .map(|(k, v)| CString::new(format!("{k}={v}")))
            .collect::<Result<Vec<CString>, _>>()
            .with_context(|| "unable to setup bash environment")?;

        // Safe to unwrap as we check for ebuild presence above
        let args = Self::build_args(package.ebuild.as_ref().unwrap());

        // SAFETY: We transfer ownership of the pipe ends to Ipc,
        // so they must not be used elsewhere after this call.
        let ipc = unsafe { IpcHandler::new(parent_writer, parent_reader) };

        let pid = posix_spawn(BASH_BINARY_PATH, &actions, &attrs, &args, &env)
            .with_context(|| "posix_spawn failed")?;
        Ok(Self {
            package,
            phase,
            pid,
            ipc,
        })
    }

    pub fn shutdown(&mut self) -> Result<()> {
        match waitpid(self.pid, Some(WaitPidFlag::WNOHANG))? {
            WaitStatus::StillAlive => {
                let _ = kill(self.pid, Signal::SIGTERM);
                let _ = waitpid(self.pid, None);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Creates environment variables for the given `package` according to PMS section 11.1.
    /// The caller must ensure the `package` has an associated ebuild set.
    fn create_ebuild_env(package: &Package) -> Result<HashMap<String, String>> {
        // Safe to unwrap as we check for ebuild presence in `new()`
        let ebuild = package.ebuild.as_ref().unwrap();
        let env = vec![
            (
                "BASH_COMPAT".to_owned(),
                ebuild.eapi.supported_bash_version()?,
            ),
            // Force invalid paths for bashrc and bash_env to avoid sourcing user files.
            ("BASHRC".to_owned(), "/dev/null".to_owned()),
            ("BASH_ENV".to_owned(), "/dev/null".to_owned()),
            // Ebuild variables
            ("P".to_owned(), package.p()),
            ("PF".to_owned(), package.pf()),
            ("PN".to_owned(), package.pn()),
            ("CATEGORY".to_owned(), package.category()),
            ("PV".to_owned(), package.pv()),
            ("PR".to_owned(), package.pr()),
            ("PVR".to_owned(), package.pvr()),
            (
                "EBUILD".to_owned(),
                ebuild.path.to_str().unwrap().to_owned(),
            ),
        ];
        Ok(HashMap::from_iter(env))
    }

    /// Builds the list of `args` to be passed to bash for the ebuild process.
    /// Also sets shell options depending on the EAPI.
    /// See https://www.gnu.org/software/bash/manual/html_node/The-Shopt-Builtin.html
    fn build_args(ebuild: &Ebuild) -> Vec<CString> {
        // The "patsub_replacement" and "globskipdots" options were introduced
        // by bash-5.2. Both are default-enabled and change the behavior of
        // bash in a manner that is backwards-incompatible. Setting BASH_COMPAT
        // has no effect on either option. Hence, ensure that both are disabled
        // until such time as a future EAPI not only requires >=5.2, but also
        // mandates that the options be enabled.
        //
        // https://bugs.gentoo.org/881383
        // https://bugs.gentoo.org/907061
        // https://bugs.gentoo.org/946193
        // https://bugs.gentoo.org/946179
        let mut args = vec![
            "BASH_BINARY_PATH",
            "+O",
            "patsub_replacement",
            "+O",
            "globskipdots",
        ];
        if ebuild.eapi.enables_failglob() {
            args.extend(vec!["-O", "failglob"]);
        }
        args.extend_from_slice(&[
            "-c",
            "shopt; ls -la /proc/$$/fd; printenv; whoami; read l <&3; echo \"bash got: $l\" >&6",
        ]);

        // Ok to unwrap here as CString::from_str only fails on interior null bytes
        args.iter().map(|s| CString::from_str(s).unwrap()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eapi::Eapi;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_ebuild_process_ipc() {
        let pkg = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("7.0.174", None, Some("1")).unwrap(),
        )
        .unwrap()
        .with_ebuild(Ebuild {
            path: "/dev/null".into(),
            eapi: Eapi::new("8").unwrap(),
        });
        let mut proc = EbuildProcess::new(&pkg, EbuildPhase::Metadata).unwrap();
        proc.ipc.send("hello").unwrap();
        let resp = proc.ipc.recv().unwrap();
        assert_eq!(resp, Some("bash got: hello".into()));
        proc.shutdown().unwrap();
    }
}
