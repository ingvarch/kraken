use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Parser;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Select};
use kraken::release::{Plan, Release};
use kraken::version::{Level, bump};
use semver::Version;

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
    #[arg(long)]
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
    print_summary(&release);
    if release.remote_branch.is_some() {
        println!("Checking tags on origin...");
        if let Some(warning) = release.check_origin_tags()? {
            eprintln!("warning: {warning}");
        }
    }

    let asks_push = !cli.yes && !cli.dry_run && release.remote_branch.is_some();
    if (cli.level.is_none() || asks_push) && !in_terminal() {
        bail!("no terminal to ask in; pass patch, minor or major and --yes");
    }
    let level = match cli.level {
        Some(level) => level,
        None => match ask_level(&release.current)? {
            Some(level) => level,
            None => return Ok(()),
        },
    };
    let plan = release.plan(level)?;

    if cli.dry_run {
        print_dry_run(&release, &plan);
        return Ok(());
    }

    release.apply(&plan)?;
    if let Some(message) = &plan.commit {
        println!("Updated {}", plan.files.join(", "));
        print_untracked(&plan);
        println!("Committed \"{message}\"");
    }
    println!("Tagged {}", plan.tag);

    let Some(push) = describe_push(&release, &plan) else {
        println!("No origin remote: nothing to push");
        return Ok(());
    };
    if cli.yes || ask(&format!("Push {push}?"))? {
        release.push(&plan)?;
        println!("Pushed {push}");
    }
    Ok(())
}

/// "main and v1.2.4 to origin", naming the remote branch when it differs.
fn describe_push(release: &Release, plan: &Plan) -> Option<String> {
    let remote_branch = release.remote_branch.as_ref()?;
    let branch = &release.branch;
    let tag = &plan.tag;
    Some(if *remote_branch == *branch {
        format!("{branch} and {tag} to origin")
    } else {
        format!("{branch} to origin/{remote_branch} and {tag} to origin")
    })
}

fn print_untracked(plan: &Plan) {
    if !plan.untracked.is_empty() {
        println!(
            "Not tracked by git, left out of the commit: {}",
            plan.untracked.join(", ")
        );
    }
}

fn in_terminal() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

fn print_summary(release: &Release) {
    if release.manifests.is_empty() {
        println!("No version files, using git tags");
    } else {
        let names: Vec<_> = release
            .manifests
            .iter()
            .map(|manifest| manifest.kind.file_name())
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
    if let Some(push) = describe_push(release, plan) {
        println!("  push {push}");
    }
    print_untracked(plan);
}

fn ask_level(current: &Version) -> Result<Option<Level>> {
    let items: Vec<String> = Level::ALL
        .iter()
        .map(|&level| format!("{level:<5}  {}", bump(current, level)))
        .collect();
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Next version")
        .items(&items)
        .default(0)
        .interact_opt()?;
    Ok(choice.map(|index| Level::ALL[index]))
}

fn ask(question: &str) -> Result<bool> {
    Ok(Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(question)
        .default(true)
        .interact()?)
}
