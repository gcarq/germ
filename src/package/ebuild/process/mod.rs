mod ipc;

use crate::consts::BASH_BINARY_PATH;
use crate::package::Package;
use crate::package::ebuild::process::ipc::IpcHandler;
use anyhow::{Context, Result, anyhow};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::Signal;
use nix::sys::signal::kill;
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
        let (_child_reader, parent_writer) = pipe().with_context(|| "unable to create pipe")?;
        // IPC pipe for child -> parent
        let (parent_reader, _child_writer) = pipe().with_context(|| "unable to create pipe")?;
        let parent_writer = mem::ManuallyDrop::new(parent_writer);
        let parent_reader = mem::ManuallyDrop::new(parent_reader);

        let mut actions = PosixSpawnFileActions::init()
            .with_context(|| "unable to init posix spawn file actions")?;
        let attrs =
            PosixSpawnAttr::init().with_context(|| "unable to init posix spawn attributes")?;

        actions
            .add_close(parent_writer.as_raw_fd())
            .with_context(|| "unable to add posix spawn close handler")?;
        actions
            .add_close(parent_reader.as_raw_fd())
            .with_context(|| "unable to add posix spawn close handler")?;

        let env = Self::create_ebuild_env(package)?
            .iter()
            .map(|(k, v)| CString::new(format!("{k}={v}")))
            .collect::<Result<Vec<CString>, _>>()
            .with_context(|| "unable to setup bash environment")?;

        // TODO: replace me, this is only for debugging purposes
        let args = [
            CString::from_str(BASH_BINARY_PATH)?,
            CString::from_str("-c")?,
            CString::from_str(
                "ls -la /proc/$$/fd; printenv; whoami; read l <&3; echo \"bash got: $l\" >&6",
            )?,
        ];
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

    /// Creates the ebuild environment variables according to PMS section 11.1.
    fn create_ebuild_env(package: &Package) -> Result<HashMap<String, String>> {
        let ebuild = match &package.ebuild {
            Some(ebuild) => ebuild,
            None => return Err(anyhow!("no ebuild found for package: {package}")),
        };
        let ebuild_path = ebuild.path.to_str().unwrap().to_owned();
        let env = vec![
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
            ("EBUILD".to_owned(), ebuild_path),
        ];
        Ok(HashMap::from_iter(env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eapi::Eapi;
    use crate::package::ebuild::Ebuild;
    use crate::package::version::PackageVersion;

    #[test]
    fn test_ebuild_process_ipc() {
        let pkg = Package::new(
            "app-editors",
            "vim",
            PackageVersion::new("1.0.0", None, None).unwrap(),
        )
        .unwrap()
        .with_ebuild(Ebuild {
            path: "/dev/null".into(),
            eapi: Eapi::default(),
        });
        let mut proc = EbuildProcess::new(&pkg, EbuildPhase::Metadata).unwrap();
        proc.ipc.send("hello").unwrap();
        let resp = proc.ipc.recv().unwrap();
        assert_eq!(resp, Some("bash got: hello".into()));
        proc.shutdown().unwrap();
    }
}
