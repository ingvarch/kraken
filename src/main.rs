use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{ArgGroup, Parser, Subcommand, ValueEnum};
use kraken::changelog::{self, Setup};
use kraken::commit::Tally;
use kraken::release::{Current, Next, Plan, Release, push_target};
use kraken::ui::{self, App, Choice, Facts};
use kraken::version::{Level, Pre, next, pre_release_level, suggestion_warning};
use ratatui::crossterm::style::Stylize;
use semver::Version;

/// Bump the project version, tag the release and push it.
#[derive(Parser)]
#[command(
    version,
    args_conflicts_with_subcommands = true,
    disable_help_subcommand = true,
    group(ArgGroup::new("target").args(["level", "pre"]).multiple(true))
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Part of the version to bump, or auto to take it from the commits; asks when omitted
    #[arg(value_enum)]
    level: Option<LevelArg>,

    /// Make a pre-release: alpha, beta or rc
    #[arg(long, value_enum, value_name = "LABEL")]
    pre: Option<Pre>,

    /// Answer yes to every question, including the push to origin
    #[arg(short, long, requires = "target")]
    yes: bool,

    /// Show what would happen and change nothing
    #[arg(long, requires = "target")]
    dry_run: bool,

    /// Leave CHANGELOG.md out of this release
    #[arg(long)]
    no_changelog: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum LevelArg {
    Patch,
    Minor,
    Major,
    Auto,
}

#[derive(Subcommand)]
enum Command {
    /// Print the project's current version and change nothing
    Version {
        /// Print the version the next release gets, with the level taken from the commits
        #[arg(long)]
        next: bool,

        /// Make the next version a pre-release: alpha, beta or rc
        #[arg(long, value_enum, value_name = "LABEL", requires = "next")]
        pre: Option<Pre>,
    },
    /// Create CHANGELOG.md, which every release then adds a section to
    Changelog,
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
    match cli.command {
        Some(Command::Version { next, pre }) => return print_version(next, pre),
        Some(Command::Changelog) => return create_changelog(),
        None => {}
    }
    let release = Release::prepare(&std::env::current_dir()?, !cli.no_changelog)?;
    let level = match cli.level {
        Some(arg) => Some(level(arg, &release, cli.pre)?),
        None if cli.pre.is_some() => pre_release_level(&release.current),
        None => None,
    };
    if let Some(level) = level {
        next(&release.current, level, cli.pre)?;
    }
    if opens_panel(cli, io::stdin().is_terminal(), io::stdout().is_terminal()) {
        return release_in_panel(&release, level, cli.pre);
    }

    print_summary(&release, cli.pre);
    if let (Some(LevelArg::Auto), Some(level)) = (cli.level, level) {
        println!("{}", level_line(level, &release.current, &release.tally()));
    }
    if let Some(warning) = check_origin(&release)? {
        eprintln!("warning: {warning}");
    }
    let Some(level) = level else {
        if cli.pre.is_some() {
            bail!(
                "--pre without a level continues a pre-release, and {} is not one; pass patch, minor, major or auto",
                release.current
            );
        }
        bail!(
            "choosing the level needs stdin and stdout to be a terminal; pass patch, minor, major or auto"
        );
    };
    if !cli.yes && !cli.dry_run && release.remote_branch.is_some() {
        bail!("asking about the push needs stdin and stdout to be a terminal; pass --yes to push");
    }
    if let Some(warning) = suggestion_warning(level, release.suggested()) {
        eprintln!("warning: {warning}");
    }
    let plan = release.plan(level, cli.pre)?;

    if cli.dry_run {
        print_dry_run(&release, &plan);
        return Ok(());
    }

    finish(&release, &plan, true)
}

fn level(arg: LevelArg, release: &Release, pre: Option<Pre>) -> Result<Level> {
    match arg {
        LevelArg::Patch => Ok(Level::Patch),
        LevelArg::Minor => Ok(Level::Minor),
        LevelArg::Major => Ok(Level::Major),
        LevelArg::Auto => release.auto_level(pre),
    }
}

/// `Level: minor` for the level `suggest` gave, with the reason when a breaking
/// change bumps only minor in 0.x.
fn level_line(level: Level, current: &Version, tally: &Tally) -> String {
    if tally.breaking > 0 && current.major == 0 {
        format!("Level: {level} (breaking changes bump minor in 0.x)")
    } else {
        format!("Level: {level}")
    }
}

fn print_version(next: bool, pre: Option<Pre>) -> Result<()> {
    if next {
        return print_next_version(pre);
    }
    let current = Current::of(&std::env::current_dir()?)?;
    if let Some(warning) = current.warning() {
        eprintln!("warning: {warning}");
    }
    println!("{}", current.version);
    Ok(())
}

fn print_next_version(pre: Option<Pre>) -> Result<()> {
    let next = Next::of(&std::env::current_dir()?, pre)?;
    if let Some(warning) = next.warning() {
        eprintln!("warning: {warning}");
    }
    eprintln!(
        "Changes: {}",
        next.commits.summary(next.latest_tag.as_deref())
    );
    eprintln!("{}", level_line(next.level, &next.current, &next.commits));
    println!("{}", next.version);
    Ok(())
}

fn create_changelog() -> Result<()> {
    let file = changelog::FILE;
    match changelog::create(&std::env::current_dir()?)? {
        Setup::Created => {
            println!("Created {file}. Commit it, and every release adds a section to it.")
        }
        Setup::Ready => println!(
            "{file} has the kraken line. Once it is committed, every release adds a section to it."
        ),
    }
    Ok(())
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
fn release_in_panel(release: &Release, level: Option<Level>, pre: Option<Pre>) -> Result<()> {
    let mut warnings = release.warnings(pre);
    warnings.extend(check_origin(release)?);
    let facts = Facts {
        version_files: release
            .manifests
            .iter()
            .map(|manifest| manifest.path.clone())
            .collect(),
        current: release.current.clone(),
        latest_tag: release.latest_tag().map(str::to_string),
        changes_since: release.final_tag().map(str::to_string),
        branch: release.branch.clone(),
        remote_branch: release.remote_branch.clone(),
        warnings,
        commits: release.tally(),
    };
    let choices = Level::ALL
        .iter()
        .filter_map(|&level| {
            let version = next(&release.current, level, pre).ok()?;
            Some(Choice {
                level,
                version,
                plan: release.plan(level, pre).map_err(|err| format!("{err:#}")),
            })
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

fn print_summary(release: &Release, pre: Option<Pre>) {
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
    println!("Changes: {}", release.tally().summary(release.final_tag()));
    for warning in release.warnings(pre) {
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
    for (spelling, files) in &plan.other_spellings {
        println!(
            "  write {spelling} to {}: the PEP 440 spelling",
            files.join(", ")
        );
    }
    if let Some(version) = &plan.changelog_waits {
        println!(
            "  leave {} alone: {version} gets the section",
            changelog::FILE
        );
    }
    if let Some(section) = &plan.changelog {
        println!("  add to {}:\n", changelog::FILE);
        for line in section.markdown().lines() {
            if line.is_empty() {
                println!();
            } else {
                println!("    {line}");
            }
        }
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
    fn level_line_explains_minor_for_a_breaking_change_in_0x() {
        let tally = Tally {
            breaking: 1,
            ..Tally::default()
        };
        let line = level_line(Level::Minor, &Version::new(0, 3, 0), &tally);

        assert_eq!(line, "Level: minor (breaking changes bump minor in 0.x)");
        assert_eq!(
            level_line(Level::Major, &Version::new(1, 3, 0), &tally),
            "Level: major"
        );
        let features = Tally {
            features: 1,
            ..Tally::default()
        };
        assert_eq!(
            level_line(Level::Minor, &Version::new(0, 3, 0), &features),
            "Level: minor"
        );
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

    #[test]
    fn auto_is_a_level() {
        let auto = cli(&["auto", "--yes"]);

        assert!(matches!(auto.level, Some(LevelArg::Auto)));
        assert!(!opens_panel(&auto, true, true));
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("kraken").chain(args.iter().copied()))
    }

    #[test]
    fn pre_parses_without_a_level() {
        let continuing = cli(&["--pre", "rc", "--yes"]);

        assert!(continuing.level.is_none());
        assert_eq!(continuing.pre, Some(Pre::Rc));
        assert!(!opens_panel(&continuing, true, true));
        assert_eq!(cli(&["minor", "--pre", "beta"]).pre, Some(Pre::Beta));
    }

    #[test]
    fn yes_needs_a_level_or_pre() {
        for flag in ["--yes", "--dry-run"] {
            let err = parse(&[flag]).err().expect("must fail").to_string();
            assert!(err.contains("<LEVEL|--pre <LABEL>>"), "{err}");
            assert!(parse(&[flag, "--pre", "rc"]).is_ok());
            assert!(parse(&[flag, "patch"]).is_ok());
        }
    }

    #[test]
    fn pre_takes_only_alpha_beta_and_rc() {
        let err = parse(&["--pre", "dev"])
            .err()
            .expect("must fail")
            .to_string();
        assert!(err.contains("[possible values: alpha, beta, rc]"), "{err}");
    }

    #[test]
    fn auto_opens_the_panel_in_a_terminal() {
        assert!(opens_panel(&cli(&["auto"]), true, true));
    }
}
