//! P11 affected verification through the existing pinned Runner.
//! Exact registered labels only; setup and prior results are never replayed.
use xtask::runner::{RunKind, Runner, Status};
use xtask::{Repo, Subject};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let kind = match args.first().map(String::as_str) {
        Some("check") => RunKind::Check,
        Some("test") => RunKind::Test,
        _ => return Err("usage: p11_affected check|test exact-label...".into()),
    };
    let labels = &args[1..];
    if labels.is_empty() || labels.iter().any(|label| label.starts_with("setup/")) {
        return Err(
            "select at least one affected registered label; setup is already prepared".into(),
        );
    }
    let repo = Repo::discover();
    let manifest = xtask::manifest::Manifest::load(&repo.manifest())?;
    let mut commands = manifest.plan(Subject::Phase(11), kind)?;
    commands.retain(|command| labels.contains(&command.label));
    if commands.len() != labels.len() || !commands.iter().map(|c| &c.label).eq(labels.iter()) {
        return Err("unknown, duplicate or out-of-order affected label".into());
    }
    let env = xtask::env::Environment::prepare(&repo)?;
    let runner = Runner {
        repo: &repo,
        manifest: &manifest,
        env: &env,
        subject: Subject::Phase(11),
    };
    let record = runner.run(kind, &commands, &|_| Ok(()))?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    if record.status != Status::Passed {
        return Err("affected P11 run failed; retain raw record".into());
    }
    Ok(())
}
