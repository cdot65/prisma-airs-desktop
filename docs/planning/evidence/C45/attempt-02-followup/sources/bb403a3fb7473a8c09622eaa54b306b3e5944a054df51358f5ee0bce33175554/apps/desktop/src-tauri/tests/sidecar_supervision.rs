//! C19 sidecar supervision: the production OS launcher (`OsLauncher`)
//! spawns with exactly the environment the core gives it, piped stdin and
//! stdout and no stderr, in its own process group; exit is observed without
//! reaping; killing the group takes descendants with it; nothing is
//! signalled once reaped; dropping an unreaped sidecar kills and reaps it.
#![cfg(target_os = "macos")]
// Process-table checks (kill 0, getpgid) are libc FFI.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use desktop_core::harness::sidecar::{LaunchSpec, Sidecar, SidecarLauncher};
use desktop_host::sidecar::OsLauncher;

fn spec(executable: &str, args: &[&str], env: &[(&str, &str)]) -> LaunchSpec {
    LaunchSpec {
        executable: PathBuf::from(executable),
        args: args.iter().map(OsString::from).collect(),
        env: env
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect(),
        cwd: std::env::temp_dir().canonicalize().expect("fixture"),
    }
}

/// Whether `pid` names a process (a zombie included).
fn exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_all(child: &mut Box<dyn Sidecar>) -> String {
    let mut out = String::new();
    child
        .take_stdout()
        .expect("fixture")
        .read_to_string(&mut out)
        .expect("fixture");
    out
}

#[test]
fn nothing_is_inherited_and_stderr_is_discarded() {
    // SAFETY: test-only variable set before any thread reads it.
    unsafe { std::env::set_var("AIRS_C19_INHERITED_CANARY", "must-not-appear") };
    let mut child = OsLauncher
        .launch(&spec(
            "/usr/bin/env",
            &[],
            &[("PATH", "/usr/bin:/bin"), ("LANG", "en_US.UTF-8")],
        ))
        .expect("fixture");
    let out = read_all(&mut child);
    let mut lines: Vec<&str> = out.lines().collect();
    lines.sort();
    assert_eq!(lines, ["LANG=en_US.UTF-8", "PATH=/usr/bin:/bin"]);
    wait_until("env to exit", || child.exited().expect("fixture"));
    assert_eq!(child.reap().expect("fixture"), Some(0));

    let mut child = OsLauncher
        .launch(&spec(
            "/bin/sh",
            &["-c", "echo diagnostics >&2; echo out"],
            &[],
        ))
        .expect("fixture");
    assert_eq!(read_all(&mut child), "out\n");
    wait_until("sh to exit", || child.exited().expect("fixture"));
    child.reap().expect("fixture");
}

#[test]
fn closing_input_ends_a_reader_and_exit_is_seen_before_reaping() {
    let mut child = OsLauncher
        .launch(&spec("/bin/cat", &[], &[]))
        .expect("fixture");
    let pid = child.pid();
    let mut stdin = child.take_stdin().expect("fixture");
    let mut stdout = BufReader::new(child.take_stdout().expect("fixture"));
    stdin.write_all(b"frame\n").expect("fixture");
    stdin.flush().expect("fixture");
    let mut line = String::new();
    stdout.read_line(&mut line).expect("fixture");
    assert_eq!(line, "frame\n");
    assert!(!child.exited().expect("fixture"));
    drop(stdin);
    wait_until("cat to exit on end of input", || {
        child.exited().expect("fixture")
    });
    // Observed, not reaped: the zombie keeps its pid (and its group).
    assert!(child.exited().expect("fixture"));
    assert!(exists(pid), "exit observation must not reap");
    assert_eq!(child.reap().expect("fixture"), Some(0));
    assert!(!exists(pid), "reaped");
    // Nothing is signalled once reaped.
    child.kill_group();
    assert_eq!(child.reap().expect("fixture"), Some(0));
}

#[test]
fn killing_the_group_takes_descendants_with_it() {
    let mut child = OsLauncher
        .launch(&spec(
            "/bin/sh",
            &["-c", "trap '' TERM; /bin/sleep 600 & echo $!; wait"],
            &[("PATH", "/usr/bin:/bin")],
        ))
        .expect("fixture");
    let leader = child.pid();
    let mut stdout = BufReader::new(child.take_stdout().expect("fixture"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("fixture");
    let descendant: u32 = line.trim().parse().expect("fixture");
    // The descendant is in the leader's group, not ours.
    let group = unsafe { libc::getpgid(descendant as libc::pid_t) };
    assert_eq!(group, leader as libc::pid_t);
    assert_ne!(group, unsafe { libc::getpgrp() });
    assert!(!child.exited().expect("fixture"));
    child.kill_group();
    wait_until("the leader to die", || child.exited().expect("fixture"));
    assert_eq!(
        child.reap().expect("fixture"),
        Some(-1),
        "killed by a signal"
    );
    wait_until("the descendant to die", || !exists(descendant));
}

#[test]
fn dropping_an_unreaped_sidecar_kills_and_reaps_its_group() {
    let mut child = OsLauncher
        .launch(&spec(
            "/bin/sh",
            &["-c", "/bin/sleep 600 & echo $!; wait"],
            &[],
        ))
        .expect("fixture");
    let leader = child.pid();
    let mut stdout = BufReader::new(child.take_stdout().expect("fixture"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("fixture");
    let descendant: u32 = line.trim().parse().expect("fixture");
    drop(child);
    assert!(!exists(leader), "reaped on drop");
    wait_until("the descendant to die", || !exists(descendant));
}

#[test]
fn only_absolute_paths_are_launched() {
    assert!(OsLauncher.launch(&spec("env", &[], &[])).is_err());
    let mut relative = spec("/usr/bin/env", &[], &[]);
    relative.cwd = PathBuf::from("tmp");
    assert!(OsLauncher.launch(&relative).is_err());
    assert!(
        OsLauncher
            .launch(&spec("/nonexistent/airs-harness", &[], &[]))
            .is_err()
    );
}
