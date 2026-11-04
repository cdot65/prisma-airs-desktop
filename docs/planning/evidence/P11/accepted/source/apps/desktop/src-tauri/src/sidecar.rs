//! The OS process adapter for the native Harness engine (C19, DESIGN
//! "Validation and remaining gates"): desktop-core decides what runs and
//! when it stops; this owns spawning, the process group and reaping.
//!
//! A sidecar starts with no inherited environment (the core supplies all of
//! it), piped stdin and stdout, no stderr (raw engine diagnostics never
//! reach a view or a log) and its own process group. Exit is observed
//! without reaping (`waitid` with `WNOWAIT`), so the group can still be
//! killed while its leader is a zombie and its identifier cannot have been
//! reused. Once reaped, nothing is ever signalled again. Dropping a sidecar
//! that was not reaped kills its group and reaps it.

// waitid and kill are FFI.
#![allow(unsafe_code)]

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

use desktop_core::harness::sidecar::{LaunchSpec, Sidecar, SidecarLauncher};

/// Spawns sidecars as their own process groups.
pub struct OsLauncher;

impl SidecarLauncher for OsLauncher {
    fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn Sidecar>> {
        if !spec.executable.is_absolute() || !spec.cwd.is_absolute() {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        let child = Command::new(&spec.executable)
            .args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(Box::new(OsSidecar {
            child,
            reaped: None,
        }))
    }
}

struct OsSidecar {
    child: Child,
    /// The exit code once reaped (signal deaths read as -1).
    reaped: Option<i32>,
}

impl OsSidecar {
    fn group(&self) -> Option<libc::pid_t> {
        libc::pid_t::try_from(self.child.id())
            .ok()
            .filter(|pid| *pid > 1)
    }
}

impl Sidecar for OsSidecar {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
        self.child
            .stdin
            .take()
            .map(|s| Box::new(s) as Box<dyn Write + Send>)
    }

    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
        self.child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>)
    }

    fn exited(&mut self) -> std::io::Result<bool> {
        if self.reaped.is_some() {
            return Ok(true);
        }
        let pid = self.group().ok_or(std::io::ErrorKind::InvalidInput)?;
        // SAFETY: `info` is a zeroed siginfo_t the call writes; the pid is
        // our own unreaped child, so it names no other process.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(info.si_pid != 0)
    }

    fn kill_group(&mut self) {
        if self.reaped.is_some() {
            return;
        }
        if let Some(pid) = self.group() {
            // SAFETY: the child was started as the leader of its own group
            // and is not reaped, so `-pid` names that group only. Never our
            // own group (the child's group id is its pid, never ours).
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }

    fn reap(&mut self) -> std::io::Result<Option<i32>> {
        if let Some(code) = self.reaped {
            return Ok(Some(code));
        }
        Ok(self.child.try_wait()?.map(|status| {
            let code = status.code().unwrap_or(-1);
            self.reaped = Some(code);
            code
        }))
    }
}

impl Drop for OsSidecar {
    fn drop(&mut self) {
        if self.reaped.is_none() {
            self.kill_group();
            let _ = self.child.wait();
        }
    }
}
