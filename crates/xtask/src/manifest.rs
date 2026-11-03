//! The approved command manifest (`docs/architecture/checks.json`).
//!
//! Parsing is strict: unknown fields fail, every command is a non-empty argv
//! array whose program is on a fixed allowlist, and placeholders may appear
//! only where the controller substitutes validated values. Completion state is
//! never read from the manifest; it lives in the append-only ledger.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::runner::RunKind;
use crate::{Result, Subject, invalid, read_json};

/// Programs a manifest command may start.
const PROGRAMS: [&str; 2] = ["cargo", "pnpm"];

pub type Argv = Vec<String>;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub status: String,
    pub target: String,
    pub fixture_only_features: Vec<String>,
    /// Materialize locked inputs before a run's fingerprint is taken.
    pub setup_commands: Vec<Argv>,
    pub common_commands: Vec<Argv>,
    pub controller: String,
    pub shared_source_commands: Vec<Argv>,
    pub artifact_build: Argv,
    pub chunks: BTreeMap<String, Chunk>,
    pub phases: BTreeMap<String, Vec<String>>,
    /// Integrated fixture build for a phase's cumulative native acceptance.
    pub phase_native_prepare: Argv,
    pub invariants: Vec<String>,
    pub requested_production_features: Vec<String>,
    pub effective_features: String,
    pub controller_commands: BTreeMap<String, Argv>,
    pub reviewer: Reviewer,
    pub shared_lifecycle_contract: String,
    pub shared_source_required_chunks: Vec<String>,
    pub shared_source_requirement: String,
    pub environment_contract: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub rust_target: String,
    pub fixture_command: Argv,
    pub native_prepare: Argv,
    pub native_acceptance: String,
    pub live_requirement: String,
    /// D058: a documentation-only chunk (C27) has no application change, so
    /// no native preparation or acceptance applies and owner acceptance below
    /// 9/10 is refused. Every other chunk keeps the full gate.
    #[serde(default)]
    pub docs_only: bool,
    #[serde(default)]
    pub frontend_commands: Vec<Argv>,
    #[serde(default)]
    pub host_commands: Vec<Argv>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reviewer {
    pub decision: String,
    pub identity: String,
    pub executable: String,
    pub verdict_schema: String,
    pub threshold: BTreeMap<String, u8>,
    pub preferred_transport: PreferredTransport,
    pub fallback_transport: FallbackTransport,
    pub wait: Wait,
    pub freeze: String,
    pub expiry: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferredTransport {
    pub kind: String,
    pub thread: String,
    pub argv: Argv,
    pub submit_timeout_seconds: u64,
    pub requires: String,
    pub forbidden: Vec<String>,
    pub note: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackTransport {
    pub kind: String,
    pub argv: Argv,
    pub stdin: String,
    pub capture: Vec<String>,
    pub forbidden: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    pub poll_seconds: u64,
    pub max_wait_seconds: u64,
    pub progress_every_seconds: u64,
}

/// One concrete command the controller will run for a subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Stable label used in evidence (`common/3`, `fixture`, `host/1`, ...).
    pub label: String,
    pub argv: Argv,
    /// Whether the command must report at least one executed test.
    pub expects_tests: bool,
}

pub const DIMENSIONS: [&str; 5] = [
    "architecture",
    "behavior",
    "authority_security",
    "validation",
    "overall",
];

impl Manifest {
    pub fn load(path: &Path) -> Result<Self> {
        let manifest: Self = read_json(path)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported checks.json schema_version"));
        }
        if self.target != "aarch64-apple-darwin" {
            return Err(invalid("checks.json target must be aarch64-apple-darwin"));
        }
        for n in 1..=Subject::LAST_CHUNK {
            let id = Subject::Chunk(n).to_string();
            if !self.chunks.contains_key(&id) {
                return Err(invalid(format!("checks.json is missing chunk {id}")));
            }
        }
        if self.chunks.len() != usize::from(Subject::LAST_CHUNK) {
            return Err(invalid("checks.json must define exactly C01-C45"));
        }
        for (id, chunk) in &self.chunks {
            let fixed = Subject::parse(id)?.is_docs_only();
            if chunk.docs_only != fixed {
                return Err(invalid(format!(
                    "{id} docs_only must be {fixed}: the documentation-only policy (D058) is fixed in the controller and cannot be changed from checks.json"
                )));
            }
            if chunk.docs_only && !chunk.native_acceptance.starts_with("not applicable") {
                return Err(invalid(format!(
                    "{id} is docs_only but declares native acceptance; a documentation-only chunk states 'not applicable'"
                )));
            }
        }
        let mut phase_members = Vec::new();
        for n in 1..=Subject::LAST_PHASE {
            let id = Subject::Phase(n).to_string();
            let members = self
                .phases
                .get(&id)
                .ok_or_else(|| invalid(format!("checks.json is missing phase {id}")))?;
            phase_members.extend(members.iter().cloned());
        }
        let expected: Vec<String> = (1..=Subject::LAST_CHUNK)
            .map(|n| Subject::Chunk(n).to_string())
            .collect();
        if phase_members != expected || self.phases.len() != usize::from(Subject::LAST_PHASE) {
            return Err(invalid("phases must partition C01-C45 in order"));
        }
        for argv in self.all_static_commands() {
            check_argv(argv)?;
        }
        for (name, argv) in &self.controller_commands {
            check_argv(argv).map_err(|e| invalid(format!("controller command {name}: {e}")))?;
        }
        for dimension in DIMENSIONS {
            if self.reviewer.threshold.get(dimension) != Some(&9) {
                return Err(invalid(format!(
                    "reviewer threshold for {dimension} must be exactly 9 (D019/D026)"
                )));
            }
        }
        if self.reviewer.threshold.len() != DIMENSIONS.len() {
            return Err(invalid("reviewer threshold has unexpected dimensions"));
        }
        if self.reviewer.identity != "codex" {
            return Err(invalid("reviewer identity must be codex (D026)"));
        }
        self.check_transports()
    }

    fn check_transports(&self) -> Result<()> {
        let preferred = &self.reviewer.preferred_transport;
        let fallback = &self.reviewer.fallback_transport;
        for argv in [&preferred.argv, &fallback.argv] {
            if argv.first().map(String::as_str) != Some("{codex}") {
                return Err(invalid(
                    "reviewer argv must start with the resolved {codex}",
                ));
            }
        }
        let banned = |argv: &Argv, forbidden: &[String]| {
            argv.iter()
                .any(|a| forbidden.iter().any(|f| a == f || a.contains(f.as_str())))
        };
        if banned(&preferred.argv, &preferred.forbidden)
            || banned(&fallback.argv, &fallback.forbidden)
        {
            return Err(invalid("reviewer argv contains a forbidden flag"));
        }
        if !preferred
            .argv
            .windows(2)
            .any(|w| w[0] == "--thread" && w[1] == preferred.thread)
        {
            return Err(invalid("preferred transport must address its exact thread"));
        }
        for required in [
            "--sandbox",
            "read-only",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--output-schema",
        ] {
            if !fallback.argv.iter().any(|a| a == required) {
                return Err(invalid(format!("fallback transport must pass {required}")));
            }
        }
        Ok(())
    }

    fn all_static_commands(&self) -> impl Iterator<Item = &Argv> {
        self.setup_commands
            .iter()
            .chain(self.common_commands.iter())
            .chain(self.shared_source_commands.iter())
            .chain(std::iter::once(&self.artifact_build))
            .chain(std::iter::once(&self.phase_native_prepare))
            .chain(self.chunks.values().flat_map(|c| {
                std::iter::once(&c.fixture_command)
                    .chain(std::iter::once(&c.native_prepare))
                    .chain(c.frontend_commands.iter())
                    .chain(c.host_commands.iter())
            }))
    }

    pub fn chunk(&self, subject: Subject) -> Result<&Chunk> {
        self.chunks
            .get(&subject.to_string())
            .ok_or_else(|| invalid(format!("no manifest entry for {subject}")))
    }

    /// Chunks belonging to a phase, in order.
    pub fn phase_chunks(&self, phase: Subject) -> Result<Vec<Subject>> {
        self.phases
            .get(&phase.to_string())
            .ok_or_else(|| invalid(format!("no phase {phase}")))?
            .iter()
            .map(|c| Subject::parse(c))
            .collect()
    }

    /// Chunks whose native checks a subject's acceptance must cover: the
    /// chunk itself, or every member of a phase.
    pub fn native_members(&self, subject: Subject) -> Result<Vec<String>> {
        if subject.is_phase() {
            Ok(self
                .phase_chunks(subject)?
                .iter()
                .filter(|c| !self.docs_only(**c))
                .map(Subject::to_string)
                .collect())
        } else if self.docs_only(subject) {
            Ok(Vec::new())
        } else {
            Ok(vec![subject.to_string()])
        }
    }

    /// Whether `subject` is a documentation-only chunk (D058): no native
    /// preparation or acceptance, no owner acceptance below the threshold.
    /// Phases are never documentation-only.
    pub fn docs_only(&self, subject: Subject) -> bool {
        // The policy is fixed in `Subject`; validation guarantees checks.json
        // agrees, so a manifest can neither widen nor disable it.
        subject.is_docs_only()
    }

    /// The phase containing a chunk.
    pub fn phase_of(&self, chunk: Subject) -> Result<Subject> {
        let id = chunk.to_string();
        self.phases
            .iter()
            .find(|(_, members)| members.contains(&id))
            .map(|(phase, _)| Subject::parse(phase))
            .ok_or_else(|| invalid(format!("{chunk} belongs to no phase")))?
    }

    pub fn requires_shared_source(&self, chunk: Subject) -> bool {
        self.shared_source_required_chunks
            .contains(&chunk.to_string())
    }

    /// Commands `check` runs for a chunk, in order.
    pub fn check_commands(&self, chunk: Subject) -> Vec<Command> {
        let mut commands: Vec<Command> = self
            .common_commands
            .iter()
            .enumerate()
            .map(|(i, argv)| Command {
                label: format!("common/{}", i + 1),
                argv: argv.clone(),
                expects_tests: is_test_runner(argv),
            })
            .collect();
        if self.requires_shared_source(chunk) {
            commands.extend(
                self.shared_source_commands
                    .iter()
                    .enumerate()
                    .map(|(i, argv)| Command {
                        label: format!("shared/{}", i + 1),
                        argv: argv.clone(),
                        expects_tests: is_test_runner(argv),
                    }),
            );
        }
        commands
    }

    /// Named fixture targets `test` runs for a chunk; each must execute tests.
    pub fn test_commands(&self, chunk: Subject) -> Result<Vec<Command>> {
        let entry = self.chunk(chunk)?;
        let mut commands = vec![Command {
            label: "fixture".to_owned(),
            argv: entry.fixture_command.clone(),
            expects_tests: true,
        }];
        commands.extend(
            entry
                .host_commands
                .iter()
                .enumerate()
                .map(|(i, argv)| Command {
                    label: format!("host/{}", i + 1),
                    argv: argv.clone(),
                    expects_tests: true,
                }),
        );
        commands.extend(
            entry
                .frontend_commands
                .iter()
                .enumerate()
                .map(|(i, argv)| Command {
                    label: format!("frontend/{}", i + 1),
                    argv: argv.clone(),
                    expects_tests: true,
                }),
        );
        Ok(commands)
    }
}

fn is_test_runner(argv: &Argv) -> bool {
    argv.iter().any(|a| a == "vitest")
        || (argv.first().map(String::as_str) == Some("cargo")
            && argv.get(1).map(String::as_str) == Some("test"))
}

fn check_argv(argv: &Argv) -> Result<()> {
    let program = argv
        .first()
        .ok_or_else(|| invalid("manifest command is an empty argv"))?;
    if !PROGRAMS.contains(&program.as_str()) {
        return Err(invalid(format!(
            "manifest program {program:?} is not allowlisted"
        )));
    }
    for arg in argv {
        if arg.contains('\0') || arg.contains('\n') {
            return Err(invalid("manifest argument contains a control character"));
        }
        // Only the controller's own validated substitutions may appear.
        if arg.contains('{') && !matches!(arg.as_str(), "{chunk}" | "{phase}") {
            return Err(invalid(format!(
                "unexpected placeholder in manifest argument {arg:?}"
            )));
        }
    }
    if argv.iter().any(|a| a == "--all-features") {
        return Err(invalid(
            "--all-features is prohibited; features are explicit",
        ));
    }
    Ok(())
}

impl Manifest {
    /// The exact command plan for a run of `kind` on `subject`, with the
    /// controller's validated substitutions applied. The runner executes this
    /// plan and the completion gate requires a run record to match it.
    ///
    /// Phases rerun every chunk target from C01 through the phase's last chunk
    /// so integration regressions are exercised against one tree.
    pub fn plan(&self, subject: Subject, kind: RunKind) -> Result<Vec<Command>> {
        let chunks: Vec<Subject> = match subject {
            Subject::Chunk(_) => vec![subject],
            // P10 uses the approved C45 broad baseline and explicit coverage
            // consumer, not replay of retired C01-C44 commands (including old
            // login-Keychain probes). Subsequent reviews reuse that candidate.
            Subject::Phase(10) => self.phase_chunks(subject)?,
            Subject::Phase(_) => {
                let last = self
                    .phase_chunks(subject)?
                    .last()
                    .copied()
                    .ok_or_else(|| invalid("empty phase"))?;
                let Subject::Chunk(n) = last else {
                    return Err(invalid("phase member is not a chunk"));
                };
                (1..=n).map(Subject::Chunk).collect()
            }
        };
        let last = chunks[chunks.len() - 1];
        let mut commands = match kind {
            RunKind::Check => {
                let mut commands = self.check_commands(last);
                if subject.is_phase()
                    && chunks.iter().any(|c| self.requires_shared_source(*c))
                    && !commands.iter().any(|c| c.label.starts_with("shared/"))
                {
                    commands.extend(
                        self.check_commands(Subject::Chunk(1))
                            .into_iter()
                            .filter(|c| c.label.starts_with("shared/")),
                    );
                }
                commands
            }
            RunKind::Test => {
                let mut commands: Vec<Command> = Vec::new();
                for chunk in chunks {
                    for mut command in self.test_commands(chunk)? {
                        command.label = format!("{chunk}/{}", command.label);
                        if !commands.iter().any(|c| c.argv == command.argv) {
                            commands.push(command);
                        }
                    }
                }
                commands
            }
            RunKind::NativePrepare => vec![Command {
                label: "native-prepare".to_owned(),
                argv: if subject.is_phase() {
                    self.phase_native_prepare.clone()
                } else {
                    self.chunk(subject)?.native_prepare.clone()
                },
                expects_tests: false,
            }],
        };
        let mut planned: Vec<Command> = self
            .setup_commands
            .iter()
            .enumerate()
            .map(|(i, argv)| Command {
                label: format!("setup/{}", i + 1),
                argv: argv.clone(),
                expects_tests: false,
            })
            .collect();
        planned.append(&mut commands);
        for command in &mut planned {
            command.argv = crate::runner::substitute(&command.argv, subject)?;
        }
        Ok(planned)
    }
}

#[cfg(test)]
mod c45_plan_tests {
    #[test]
    fn p10_uses_the_current_baseline_without_private_keychain_replay() {
        let repo = crate::Repo::discover();
        let manifest = super::Manifest::load(&repo.manifest()).expect("manifest");
        let plan = manifest
            .plan(crate::Subject::Phase(10), crate::runner::RunKind::Test)
            .expect("P10 plan");
        assert!(
            plan.iter()
                .all(|c| c.label.starts_with("setup/") || c.label.starts_with("C45/"))
        );
        let host = plan
            .iter()
            .find(|c| c.argv.iter().any(|a| a == "desktop-host"))
            .expect("current Host coverage");
        assert!(host.argv.windows(2).any(|a| a
            == [
                "--skip",
                "the_classic_login_keychain_api_is_available_read_only"
            ]));
        assert!(host.argv.windows(2).any(|a| a
            == [
                "--skip",
                "records_round_trip_in_the_v2_namespace_without_touching_the_search_list"
            ]));
        assert!(
            !plan
                .iter()
                .any(|c| c.argv.iter().any(|a| a == "imports_rollback"))
        );
    }
}
