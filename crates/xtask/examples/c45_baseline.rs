//! C45 selected baseline using the existing manifest, environment and runner.
//! No new controller completion or review transport. Dependencies were prepared
//! offline before this run; setup is not repeated or claimed as newly executed.
use xtask::runner::{RunKind, Runner, Status};
use xtask::{Repo, Subject};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let repo = Repo::discover();
    let manifest = xtask::manifest::Manifest::load(&repo.manifest())?;
    let env = xtask::env::Environment::prepare(&repo)?.with("RUST_TEST_THREADS", "4");
    if args.as_slice() == ["identity"] {
        let fingerprints =
            xtask::fingerprint::all(&repo, &manifest, Subject::Chunk(45), &env.toolchain_hash())?;
        println!("{}", serde_json::to_string_pretty(&fingerprints)?);
        return Ok(());
    }
    let kind = match args.first().map(String::as_str) {
        Some("check") => RunKind::Check,
        Some("test") => RunKind::Test,
        _ => return Err("usage: c45_baseline check|test [exact-command-label]".into()),
    };
    if args.len() > 2 {
        return Err("too many selection arguments".into());
    }

    let mut commands = manifest.plan(Subject::Chunk(45), kind)?;
    commands.retain(|c| !c.label.starts_with("setup/"));
    // Check already runs the full frontend suite. The test baseline keeps truth
    // tests; duplicate execution of exactly the same command adds no coverage.
    if kind == RunKind::Test {
        let check = manifest.plan(Subject::Chunk(45), RunKind::Check)?;
        commands.retain(|c| !check.iter().any(|other| other.argv == c.argv));
    }
    if let Some(label) = args.get(1) {
        commands.retain(|c| &c.label == label);
        if commands.len() != 1 {
            return Err("unknown/duplicate approved command label".into());
        }
    }
    let runner = Runner {
        repo: &repo,
        manifest: &manifest,
        env: &env,
        subject: Subject::Chunk(45),
    };
    let record = runner.run(kind, &commands, &|_| Ok(()))?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    if record.status != Status::Passed {
        return Err("selected baseline failed; retain raw run and stop".into());
    }
    Ok(())
}
