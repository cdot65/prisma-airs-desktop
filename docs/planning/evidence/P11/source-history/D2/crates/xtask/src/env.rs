//! The environment contract for controller commands.
//!
//! Every command starts from a cleared environment. Only the preserved
//! identity variables, an explicit PATH of selected tool directories and the
//! declared nonsecret build values are set. Inherited variables that could
//! silently change compilation are refused outright, `.env` files fail the
//! run, and the resolved tool identities (version output and executable
//! hash) are recorded as dependency inputs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{Repo, Result, invalid, io_err, read_json, sha256_hex};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolchainManifest {
    schema_version: u32,
    #[allow(dead_code)]
    description: String,
    path_dirs: Vec<String>,
    tools: Vec<ToolSpec>,
    declared_env: BTreeMap<String, String>,
    preserve_unchanged: Vec<String>,
    reject_inherited: Vec<String>,
    reject_inherited_prefixes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolSpec {
    name: String,
    argv: Vec<String>,
    expect_prefix: String,
}

/// A resolved build tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolIdentity {
    pub name: String,
    pub path: String,
    pub version: String,
    pub sha256: String,
}

/// The concrete environment commands run under.
#[derive(Clone, Debug)]
pub struct Environment {
    vars: BTreeMap<String, String>,
    path_dirs: Vec<PathBuf>,
    pub tools: Vec<ToolIdentity>,
}

impl Environment {
    /// Resolves the toolchain and verifies tool versions.
    pub fn prepare(repo: &Repo) -> Result<Self> {
        let spec: ToolchainManifest = read_json(&repo.toolchain_manifest())?;
        if spec.schema_version != 1 {
            return Err(invalid("unsupported toolchain.json schema_version"));
        }
        refuse_inherited(&spec)?;
        refuse_env_files(repo)?;
        let home = std::env::var("HOME").map_err(|_| invalid("HOME must be set"))?;
        let path_dirs: Vec<PathBuf> = spec
            .path_dirs
            .iter()
            .map(|dir| expand(dir, &home, repo.root()))
            .collect();
        let mut vars = BTreeMap::new();
        for name in &spec.preserve_unchanged {
            if let Ok(value) = std::env::var(name) {
                vars.insert(name.clone(), value);
            }
        }
        for (name, value) in &spec.declared_env {
            if vars.contains_key(name) {
                return Err(invalid(format!("{name} is both preserved and declared")));
            }
            vars.insert(name.clone(), value.clone());
        }
        let joined = path_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":");
        vars.insert("PATH".to_owned(), joined);
        let mut env = Self {
            vars,
            path_dirs,
            tools: Vec::new(),
        };
        for tool in &spec.tools {
            env.tools.push(env.identify(tool, repo.root())?);
        }
        // rustc/cargo in PATH are rustup shims; also bind the selected
        // toolchain's actual executables.
        for name in ["rustc", "cargo"] {
            let argv = vec!["rustup".to_owned(), "which".to_owned(), name.to_owned()];
            let output = env
                .command(&argv, repo.root())?
                .output()
                .map_err(io_err(repo.root()))?;
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            if !output.status.success() || !path.is_file() {
                return Err(invalid(format!(
                    "rustup cannot resolve the selected {name}"
                )));
            }
            let bytes = std::fs::read(&path).map_err(io_err(&path))?;
            env.tools.push(ToolIdentity {
                name: format!("{name}-toolchain"),
                path: path.display().to_string(),
                version: String::new(),
                sha256: sha256_hex(&bytes),
            });
        }
        Ok(env)
    }

    fn identify(&self, tool: &ToolSpec, cwd: &Path) -> Result<ToolIdentity> {
        let program = self.resolve(&tool.argv[0])?;
        let output = self
            .command(&tool.argv, cwd)?
            .output()
            .map_err(io_err(&program))?;
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || !version.starts_with(&tool.expect_prefix) {
            return Err(invalid(format!(
                "{} reports {version:?}; expected prefix {:?}",
                tool.name, tool.expect_prefix
            )));
        }
        let bytes = std::fs::read(&program).map_err(io_err(&program))?;
        Ok(ToolIdentity {
            name: tool.name.clone(),
            path: program.display().to_string(),
            version,
            sha256: sha256_hex(&bytes),
        })
    }

    /// Finds a program in the selected PATH (never the caller's PATH).
    pub fn resolve(&self, program: &str) -> Result<PathBuf> {
        if program.contains('/') {
            return Err(invalid(format!("program must be a bare name: {program}")));
        }
        self.path_dirs
            .iter()
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                invalid(format!(
                    "{program} not found in the selected toolchain PATH"
                ))
            })
    }

    /// Builds a process for an argv under this environment.
    pub fn command(&self, argv: &[String], cwd: &Path) -> Result<Command> {
        let program = argv
            .first()
            .ok_or_else(|| invalid("empty argv"))
            .and_then(|p| self.resolve(p))?;
        let mut command = Command::new(program);
        command.args(&argv[1..]).current_dir(cwd).env_clear();
        for (name, value) in &self.vars {
            command.env(name, value);
        }
        Ok(command)
    }

    /// Adds a declared value for specific commands (never secrets).
    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.vars.insert(name.to_owned(), value.to_owned());
        self
    }

    /// Nonsecret effective variables recorded with each run. Identity values
    /// (HOME/USER/LOGNAME) are recorded by name only.
    pub fn recorded_vars(&self) -> BTreeMap<String, String> {
        self.vars
            .iter()
            .map(|(name, value)| {
                let shown = if matches!(name.as_str(), "HOME" | "USER" | "LOGNAME" | "TMPDIR") {
                    "<preserved>".to_owned()
                } else {
                    value.clone()
                };
                (name.clone(), shown)
            })
            .collect()
    }

    /// Hash of the toolchain identities, part of the dependency fingerprint.
    pub fn toolchain_hash(&self) -> String {
        let text = self
            .tools
            .iter()
            .map(|t| format!("{}\0{}\0{}\n", t.name, t.version, t.sha256))
            .collect::<String>();
        sha256_hex(text.as_bytes())
    }
}

fn expand(dir: &str, home: &str, repo: &Path) -> PathBuf {
    if let Some(rest) = dir.strip_prefix("$HOME") {
        PathBuf::from(format!("{home}{rest}"))
    } else if let Some(rest) = dir.strip_prefix("{repo}") {
        PathBuf::from(format!("{}{rest}", repo.display()))
    } else {
        PathBuf::from(dir)
    }
}

fn refuse_inherited(spec: &ToolchainManifest) -> Result<()> {
    // A variable already equal to its declared value (for example inside a
    // nested controller command) is not an override.
    let mut offending: Vec<String> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok())))
        .filter(|(name, value)| spec.declared_env.get(name).map(String::as_str) != value.as_deref())
        .map(|(name, _)| name)
        .filter(|name| {
            spec.reject_inherited.contains(name)
                || spec
                    .reject_inherited_prefixes
                    .iter()
                    .any(|prefix| name.starts_with(prefix.as_str()))
        })
        .collect();
    offending.sort();
    if offending.is_empty() {
        Ok(())
    } else {
        Err(invalid(format!(
            "refusing undeclared inherited overrides: {}",
            offending.join(", ")
        )))
    }
}

/// `.env` files are never read and their presence fails the run.
pub fn refuse_env_files(repo: &Repo) -> Result<()> {
    let mut dirs = vec![
        repo.root().to_path_buf(),
        repo.join("apps/desktop"),
        repo.join("apps/desktop/src-tauri"),
    ];
    if let Ok(entries) = std::fs::read_dir(repo.join("packages")) {
        dirs.extend(entries.filter_map(|e| e.ok()).map(|e| e.path()));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".env" || name.starts_with(".env.") {
                return Err(invalid(format!(
                    "unexpected environment file {}; the controller never reads .env files",
                    entry.path().display()
                )));
            }
        }
    }
    Ok(())
}
