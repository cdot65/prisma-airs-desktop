//! The seam between the Harness lifecycle (desktop-core) and the OS
//! process adapter (desktop-host, DESIGN "Validation and remaining
//! gates"). The core decides what is launched, with which environment, and
//! when it is stopped; the adapter owns spawning, the process group and
//! reaping. The presentation never reaches either.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

/// What to start: no inherited environment, piped stdin and stdout, no
/// stderr, its own process group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    /// The complete environment (nothing is inherited).
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
}

/// Starts sidecars.
pub trait SidecarLauncher: Send + Sync {
    fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn Sidecar>>;
}

/// One started process, the leader of its own process group. The adapter
/// never signals a process it has reaped, so a recycled identifier is
/// never acted on.
pub trait Sidecar: Send {
    fn pid(&self) -> u32;
    fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>>;
    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>>;
    /// Whether the leader has exited, without reaping it.
    fn exited(&mut self) -> std::io::Result<bool>;
    /// Kills every process in the group (SIGKILL). A no-op once reaped.
    fn kill_group(&mut self);
    /// Reaps the leader once it has exited. `None` while it still runs.
    fn reap(&mut self) -> std::io::Result<Option<i32>>;
}

/// No sidecars on this build or platform.
pub struct UnavailableLauncher;

impl SidecarLauncher for UnavailableLauncher {
    fn launch(&self, _: &LaunchSpec) -> std::io::Result<Box<dyn Sidecar>> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// The Harness's OS seams, provided by the host.
#[derive(Clone)]
pub struct HarnessAdapters {
    pub launcher: std::sync::Arc<dyn SidecarLauncher>,
    /// The bundled engine (None: not bundled in this build).
    pub engine: Option<super::engine::Engine>,
    /// Where ephemeral runtime homes are made (None: unavailable).
    pub runtime_root: Option<PathBuf>,
    /// Where protected history is kept (C20; None: not kept).
    pub history_root: Option<PathBuf>,
    /// Fixture builds only: a stand-in engine's pin and a shorter
    /// bootstrap bound. Shipping builds always use the compiled pin.
    #[cfg(feature = "test-fixtures")]
    pub fixture: Option<FixtureEngine>,
    /// Fixture builds only: a gate every history save passes after it is
    /// admitted and before it writes, so a test can hold a save open there
    /// (C30: a discard racing an admitted save).
    #[cfg(feature = "test-fixtures")]
    pub save_gate: Option<std::sync::Arc<std::sync::Mutex<()>>>,
}

/// A stand-in engine for fixture tests.
#[cfg(feature = "test-fixtures")]
#[derive(Clone, Debug)]
pub struct FixtureEngine {
    pub pin: super::engine::Pin,
    pub bootstrap_timeout: std::time::Duration,
}

impl HarnessAdapters {
    pub fn unavailable() -> Self {
        Self {
            launcher: std::sync::Arc::new(UnavailableLauncher),
            engine: None,
            runtime_root: None,
            history_root: None,
            #[cfg(feature = "test-fixtures")]
            fixture: None,
            #[cfg(feature = "test-fixtures")]
            save_gate: None,
        }
    }

    /// The pin a start verifies against.
    pub fn pin(&self) -> super::engine::Pin {
        #[cfg(feature = "test-fixtures")]
        if let Some(f) = &self.fixture {
            return f.pin.clone();
        }
        super::engine::pin()
    }

    pub fn bootstrap_timeout(&self) -> std::time::Duration {
        #[cfg(feature = "test-fixtures")]
        if let Some(f) = &self.fixture {
            return f.bootstrap_timeout;
        }
        super::session::BOOTSTRAP_TIMEOUT
    }
}
