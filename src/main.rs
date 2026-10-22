use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Parser;
use kraken::release::{Plan, Release, push_target};
use kraken::ui::{self, App, Choice, Facts};
use kraken::version::Level;
use ratatui::crossterm::style::Stylize;

/// Bump the project version, tag the release and push it.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Part of the version to bump; asks when omitted
    #[arg(value_enum)]
    level: Option<Level>,

    /// Answer yes to every question, including the push to origin
    #[arg(short, long, requires = "level")]
    yes: bool,

    /// Show what would happen and change nothing
    #[arg(long, requires = "level")]
    dry_run: bool,
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let release = Release::prepare(&std::env::current_dir()?)?;
    if opens_panel(cli, io::stdin().is_terminal(), io::stdout().is_terminal()) {
        return release_in_panel(&release, cli.level);
    }

    print_summary(&release);
    if let Some(warning) = check_origin(&release)? {
        eprintln!("warning: {warning}");
    }
    let Some(level) = cli.level else {
        bail!(
            "choosing the level needs stdin and stdout to be a terminal; pass patch, minor or major"
        );
    };
    if !cli.yes && !cli.dry_run && release.remote_branch.is_some() {
        bail!("asking about the push needs stdin and stdout to be a terminal; pass --yes to push");
    }
    let plan = release.plan(level)?;

    if cli.dry_run {
        print_dry_run(&release, &plan);
        return Ok(());
    }

    finish(&release, &plan, true)
}

/// Writes, commits, tags and pushes in the plain terminal, where git can ask for
/// passphrases or credentials and ctrl-c works.
fn finish(release: &Release, plan: &Plan, push: bool) -> Result<()> {
    release.apply(plan)?;
    print_applied(plan);
    match push_of(release, plan) {
        Some(target) if push => {
            println!("Pushing {target}...");
            release.push(plan)?;
            println!("Pushed {target}");
        }
        Some(_) => println!("Not pushed to origin"),
        None => println!("No origin remote: nothing to push"),
    }
    Ok(())
}

/// Asks for the level and confirmations in a full-screen panel, then releases after it closes.
fn release_in_panel(release: &Release, level: Option<Level>) -> Result<()> {
    let mut warnings: Vec<String> = release.warning().into_iter().collect();
    warnings.extend(check_origin(release)?);
    let facts = Facts {
        version_files: release
            .manifests
            .iter()
            .map(|manifest| manifest.path.clone())
            .collect(),
        current: release.current.clone(),
        latest_tag: release.latest_tag().map(str::to_string),
        branch: release.branch.clone(),
        remote_branch: release.remote_branch.clone(),
        warnings,
    };
    let choices = Level::ALL
        .iter()
        .map(|&level| Choice {
            level,
            plan: release.plan(level).map_err(|err| format!("{err:#}")),
        })
        .collect();

    let mut terminal = ratatui::try_init()?;
    let chosen = ui::run(&mut terminal, App::new(facts, choices, level));
    // Dropping the terminal shows the cursor again, before git may ask for a passphrase.
    drop(terminal);
    ratatui::try_restore()?;

    match chosen? {
        Some((plan, push)) => {
            if push {
                release_the_kraken();
            }
            finish(release, &plan, push)
        }
        None => Ok(()),
    }
}

fn release_the_kraken() {
    println!();
    for line in ui::KRAKEN.trim_matches('\n').lines() {
        println!("{}", line.cyan());
    }
    println!();
    println!("{}", ui::kraken_caption().bold());
    println!();
}

/// Returns a warning when origin does not answer; fails when origin has tags missing here.
fn check_origin(release: &Release) -> Result<Option<String>> {
    if release.remote_branch.is_none() {
        return Ok(None);
    }
    println!("Checking tags on origin...");
    release.check_origin_tags()
}

fn push_of(release: &Release, plan: &Plan) -> Option<String> {
    push_target(&release.branch, release.remote_branch.as_deref(), &plan.tag)
}

fn print_applied(plan: &Plan) {
    if let Some(message) = &plan.commit {
        println!("Updated {}", plan.files.join(", "));
        print_untracked(plan);
        println!("Committed \"{message}\"");
    }
    println!("Tagged {}", plan.tag);
}

fn print_untracked(plan: &Plan) {
    if !plan.untracked.is_empty() {
        println!(
            "Not tracked by git, left out of the commit: {}",
            plan.untracked.join(", ")
        );
    }
}

/// The panel reads keys from stdin and draws on stdout; scripts and pipes get plain output.
fn opens_panel(cli: &Cli, stdin_is_terminal: bool, stdout_is_terminal: bool) -> bool {
    !cli.yes && !cli.dry_run && stdin_is_terminal && stdout_is_terminal
}

fn print_summary(release: &Release) {
    if release.manifests.is_empty() {
        println!("No version files, using git tags");
    } else {
        let names: Vec<_> = release
            .manifests
            .iter()
            .map(|manifest| manifest.path.as_str())
            .collect();
        println!("Version files: {}", names.join(", "));
    }
    println!("Current version: {}", release.current);
    if let Some(warning) = release.warning() {
        eprintln!("warning: {warning}");
    }
}

fn print_dry_run(release: &Release, plan: &Plan) {
    println!("Dry run, nothing changed. kraken would:");
    if let Some(message) = &plan.commit {
        println!("  update {}", plan.files.join(", "));
        println!("  commit \"{message}\"");
    }
    println!("  tag {}", plan.tag);
    if let Some(push) = push_of(release, plan) {
        println!("  push {push}");
    }
    print_untracked(plan);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("kraken").chain(args.iter().copied()))
    }

    #[test]
    fn panel_needs_keys_and_a_screen() {
        assert!(opens_panel(&cli(&[]), true, true));
        assert!(opens_panel(&cli(&["minor"]), true, true));
        assert!(!opens_panel(&cli(&[]), true, false));
        assert!(!opens_panel(&cli(&[]), false, true));
    }

    #[test]
    fn yes_and_dry_run_skip_the_panel() {
        assert!(!opens_panel(&cli(&["patch", "--yes"]), true, true));
        assert!(!opens_panel(&cli(&["patch", "--dry-run"]), true, true));
    }
}
