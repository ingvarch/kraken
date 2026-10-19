use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const PACKAGE_JSON: &str = "{\n  \"name\": \"demo\",\n  \"version\": \"1.2.3\"\n}\n";
const PACKAGE_LOCK: &str = r#"{
  "name": "demo",
  "version": "1.2.3",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "demo", "version": "1.2.3" }
  }
}
"#;
const CARGO_TOML: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
const CARGO_LOCK: &str = "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n";

/// Keeps the user's git config (signing, hooks, identity) out of the tests.
fn isolate(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
}

fn check(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{cmd:?} failed:\n{}", stderr(&out));
    stdout(&out).trim().to_string()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "kraken failed:\n{}{}",
        stdout(out),
        stderr(out)
    );
}

fn assert_failed_with(out: &Output, message: &str) {
    assert!(!out.status.success(), "kraken succeeded:\n{}", stdout(out));
    assert!(stderr(out).contains(message), "{}", stderr(out));
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    check(isolate(Command::new("git").args(args).current_dir(dir)))
}

fn kraken_in(dir: &Path, args: &[&str]) -> Output {
    isolate(Command::new(env!("CARGO_BIN_EXE_kraken")).args(args))
        .current_dir(dir)
        .output()
        .unwrap()
}

fn bare_origin() -> TempDir {
    let origin = tempfile::tempdir().unwrap();
    git_in(origin.path(), &["init", "-q", "--bare"]);
    origin
}

struct Repo {
    dir: TempDir,
}

impl Repo {
    fn new() -> Repo {
        let dir = tempfile::tempdir().unwrap();
        git_in(dir.path(), &["init", "-q", "-b", "main"]);
        Repo { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path().join(name)).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        git_in(self.path(), args)
    }

    fn commit_all(&self) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "init"]);
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }

    fn tags(&self) -> Vec<String> {
        self.git(&["tag", "--list"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn tag_commit(&self, tag: &str) -> String {
        self.git(&["rev-parse", &format!("{tag}^{{commit}}")])
    }

    fn kraken(&self, args: &[&str]) -> Output {
        kraken_in(self.path(), args)
    }

    fn node(&self) {
        self.write("package.json", PACKAGE_JSON);
        self.write("package-lock.json", PACKAGE_LOCK);
        self.commit_all();
    }

    fn add_origin(&self, origin: &TempDir) {
        self.git(&["remote", "add", "origin", origin.path().to_str().unwrap()]);
    }
}

#[test]
fn node_patch_release() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.2.3"]);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.read("package-lock.json"),
        PACKAGE_LOCK.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.git(&["cat-file", "-t", "v1.2.4"]), "tag");
    assert_eq!(repo.status(), "");
    assert!(
        stdout(&out).contains("No origin remote"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn rust_minor_release_updates_cargo_lock() {
    let repo = Repo::new();
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("Cargo.lock", CARGO_LOCK);
    repo.write("src/lib.rs", "");
    repo.commit_all();

    assert_ok(&repo.kraken(&["minor", "--yes"]));

    assert_eq!(
        repo.read("Cargo.toml"),
        CARGO_TOML.replace("0.1.0", "0.2.0")
    );
    assert_eq!(
        repo.read("Cargo.lock"),
        CARGO_LOCK.replace("0.1.0", "0.2.0")
    );
    assert_eq!(repo.tags(), ["v0.2.0"]);
    assert_eq!(repo.status(), "");
}

const WORKSPACE_TOML: &str = r#"[workspace]
members = ["app", "engine"]
resolver = "2"

[workspace.package]
version = "0.1.0"
"#;
const WORKSPACE_LOCK: &str = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "engine",
]

[[package]]
name = "engine"
version = "0.1.0"
"#;

fn rust_workspace(repo: &Repo) {
    repo.write("Cargo.toml", WORKSPACE_TOML);
    repo.write("Cargo.lock", WORKSPACE_LOCK);
    repo.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion.workspace = true\nedition = \"2021\"\n\n[dependencies]\nengine = { path = \"../engine\", version = \"0.1.0\" }\n",
    );
    repo.write("app/src/lib.rs", "");
    repo.write(
        "engine/Cargo.toml",
        "[package]\nname = \"engine\"\nversion.workspace = true\nedition = \"2021\"\n",
    );
    repo.write("engine/src/lib.rs", "");
    repo.commit_all();
}

#[test]
fn rust_workspace_release_keeps_lock_format() {
    let repo = Repo::new();
    rust_workspace(&repo);

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(
        repo.read("Cargo.toml"),
        WORKSPACE_TOML.replace("0.1.0", "0.1.1")
    );
    assert_eq!(
        repo.read("Cargo.lock"),
        WORKSPACE_LOCK.replace("0.1.0", "0.1.1")
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn refuses_bump_that_breaks_workspace_requirements() {
    let repo = Repo::new();
    rust_workspace(&repo);
    let head = repo.head();

    assert_failed_with(&repo.kraken(&["minor", "--yes"]), "engine \"^0.1.0\"");

    assert_eq!(repo.read("Cargo.toml"), WORKSPACE_TOML);
    assert_eq!(repo.status(), "");
    assert_eq!(repo.head(), head);
    assert!(repo.tags().is_empty());
}

#[test]
fn python_major_release_updates_uv_lock() {
    let repo = Repo::new();
    let pyproject =
        "[project]\nname = \"demo\"\nversion = \"0.3.0\"\nrequires-python = \">=3.9\"\n";
    let lock = "version = 1\nrequires-python = \">=3.9\"\n\n[[package]]\nname = \"demo\"\nversion = \"0.3.0\"\nsource = { virtual = \".\" }\n";
    repo.write("pyproject.toml", pyproject);
    repo.write("uv.lock", lock);
    repo.commit_all();

    assert_ok(&repo.kraken(&["major", "--yes"]));

    assert_eq!(
        repo.read("pyproject.toml"),
        pyproject.replace("0.3.0", "1.0.0")
    );
    assert_eq!(repo.read("uv.lock"), lock.replace("0.3.0", "1.0.0"));
    assert_eq!(repo.tags(), ["v1.0.0"]);
    assert_eq!(repo.status(), "");
}

#[test]
fn tag_only_release_keeps_tag_style_and_adds_no_commit() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();
    repo.git(&["tag", "1.0.0"]);
    let head = repo.head();

    assert_ok(&repo.kraken(&["minor", "--yes"]));

    assert_eq!(repo.head(), head);
    assert_eq!(repo.tag_commit("1.1.0"), head);
}

#[test]
fn first_release_without_tags_or_version_files() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.tags(), ["v0.0.1"]);
}

#[test]
fn runs_from_a_subdirectory() {
    let repo = Repo::new();
    repo.write("src/index.js", "");
    repo.node();

    assert_ok(&kraken_in(&repo.path().join("src"), &["patch", "--yes"]));

    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.2.4")
    );
}

#[test]
fn allows_untracked_files() {
    let repo = Repo::new();
    repo.node();
    repo.write("notes.txt", "");

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.status(), "?? notes.txt");
}

#[test]
fn leaves_untracked_lock_out_of_the_commit() {
    let repo = Repo::new();
    repo.write(".gitignore", "Cargo.lock\n");
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("src/lib.rs", "");
    repo.commit_all();
    repo.write("Cargo.lock", CARGO_LOCK);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("Cargo.lock"),
        CARGO_LOCK.replace("0.1.0", "0.1.1")
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "Cargo.toml"
    );
    assert!(
        stdout(&out).contains("left out of the commit: Cargo.lock"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn pushes_branch_and_tag_to_origin() {
    let repo = Repo::new();
    repo.node();
    let origin = bare_origin();
    repo.add_origin(&origin);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    let head = repo.head();
    assert_eq!(git_in(origin.path(), &["rev-parse", "main"]), head);
    assert_eq!(
        git_in(origin.path(), &["rev-parse", "v1.2.4^{commit}"]),
        head
    );
    // The kraken greets people in the panel, not scripts.
    assert!(!stdout(&out).contains("RELEASE THE KRAKEN"));
}

#[test]
fn pushes_to_the_upstream_branch() {
    let repo = Repo::new();
    repo.node();
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["push", "-q", "origin", "main:release"]);
    repo.git(&["branch", "-q", "--set-upstream-to=origin/release"]);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        git_in(origin.path(), &["rev-parse", "release"]),
        repo.head()
    );
    assert_eq!(git_in(origin.path(), &["branch", "--list", "main"]), "");
    assert!(stdout(&out).contains("origin/release"), "{}", stdout(&out));
}

#[test]
fn rejected_push_keeps_release_local() {
    let repo = Repo::new();
    repo.node();
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.write("a.txt", "");
    repo.commit_all();
    repo.git(&["push", "-q", "origin", "main"]);
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    repo.write("b.txt", "");
    repo.commit_all();

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "stay local");

    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(git_in(origin.path(), &["tag", "--list"]), "");
}

#[test]
fn dry_run_changes_nothing() {
    let repo = Repo::new();
    repo.node();
    let head = repo.head();

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
    assert_eq!(repo.head(), head);
    assert!(repo.tags().is_empty());
    assert!(stdout(&out).contains("v1.2.4"), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("package-lock.json"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn warns_when_latest_tag_differs_from_version_files() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.0.0"]);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert!(stderr(&out).contains("warning"), "{}", stderr(&out));
    assert!(stderr(&out).contains("v1.0.0"), "{}", stderr(&out));
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
}

#[test]
fn refuses_version_not_above_latest_tag() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.3.0"]);

    assert_failed_with(
        &repo.kraken(&["patch", "--yes"]),
        "not higher than the latest tag v1.3.0",
    );
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}

#[test]
fn refuses_uncommitted_changes() {
    let repo = Repo::new();
    repo.node();
    repo.write("package.json", &PACKAGE_JSON.replace("demo", "changed"));

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "uncommitted changes");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_untracked_version_file() {
    let repo = Repo::new();
    repo.write("README.md", "");
    repo.commit_all();
    repo.write("package.json", PACKAGE_JSON);

    assert_failed_with(
        &repo.kraken(&["patch", "--yes"]),
        "package.json is not tracked",
    );
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}

#[test]
fn refuses_existing_tag() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.2.4"]);

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "v1.2.4 already exists");
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}

#[test]
fn refuses_disagreeing_version_files() {
    let repo = Repo::new();
    repo.write("package.json", PACKAGE_JSON);
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"1.3.0\"\n",
    );
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(&out, "package.json 1.2.3");
    assert!(
        stderr(&out).contains("Cargo.toml 1.3.0"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn restores_files_when_lock_update_fails() {
    let repo = Repo::new();
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("Cargo.lock", "not [[ toml");
    repo.write("src/lib.rs", "");
    repo.commit_all();
    let head = repo.head();

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "Cargo.lock");

    assert_eq!(repo.read("Cargo.toml"), CARGO_TOML);
    assert_eq!(repo.status(), "");
    assert_eq!(repo.head(), head);
    assert!(repo.tags().is_empty());
}

#[test]
fn restores_files_when_commit_fails() {
    let repo = Repo::new();
    repo.node();
    let hook = repo.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\necho rejected by hook >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let head = repo.head();

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "rejected by hook");

    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
    assert_eq!(repo.status(), "");
    assert_eq!(repo.head(), head);
}

#[test]
fn rolls_back_commit_when_tagging_fails() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["config", "tag.gpgSign", "true"]);
    repo.git(&["config", "gpg.program", "false"]);
    let head = repo.head();

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "git tag");

    assert_eq!(repo.head(), head);
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
    assert_eq!(repo.status(), "");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_shallow_clone() {
    let source = Repo::new();
    source.node();
    source.write("a.txt", "");
    source.commit_all();
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.path().display());
    git_in(clone.path(), &["clone", "-q", "--depth", "1", &url, "repo"]);

    assert_failed_with(
        &kraken_in(&clone.path().join("repo"), &["patch", "--yes"]),
        "shallow",
    );
}

#[test]
fn yes_needs_a_level() {
    let repo = Repo::new();
    repo.node();

    assert_failed_with(&repo.kraken(&["--yes"]), "LEVEL");
}

#[test]
fn asks_for_level_only_in_a_terminal() {
    let repo = Repo::new();
    repo.node();

    assert_failed_with(&repo.kraken(&[]), "needs stdin and stdout to be a terminal");
}

#[test]
fn asks_to_push_only_in_a_terminal() {
    let repo = Repo::new();
    repo.node();
    let origin = bare_origin();
    repo.add_origin(&origin);

    assert_failed_with(&repo.kraken(&["patch"]), "pass --yes to push");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_repository_without_commits() {
    let repo = Repo::new();
    repo.write("package.json", PACKAGE_JSON);

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "no commits");
}

#[test]
fn refuses_when_origin_has_tags_missing_here() {
    let source = Repo::new();
    source.write("main.go", "package main\n");
    source.commit_all();
    source.git(&["tag", "v1.4.0"]);
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.path().display());
    git_in(clone.path(), &["clone", "-q", "--no-tags", &url, "repo"]);
    let repo = clone.path().join("repo");

    assert_failed_with(&kraken_in(&repo, &["patch", "--yes"]), "v1.4.0");

    assert_eq!(git_in(&repo, &["tag", "--list"]), "");
}

#[test]
fn restores_files_when_git_index_is_locked() {
    let repo = Repo::new();
    repo.node();
    let head = repo.head();
    fs::write(repo.path().join(".git/index.lock"), "").unwrap();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(&out, "index.lock");
    assert!(
        stderr(&out).contains(&format!("run `git reset {head}`")),
        "{}",
        stderr(&out)
    );

    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
    assert_eq!(repo.read("package-lock.json"), PACKAGE_LOCK);
    assert_eq!(repo.head(), head);
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_detached_head() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["checkout", "-q", "--detach"]);

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "detached");
}

#[test]
fn refuses_outside_git() {
    let dir = tempfile::tempdir().unwrap();

    let out = kraken_in(dir.path(), &["patch", "--yes"]);

    assert_failed_with(&out, "not inside a git repository");
    assert!(
        stderr(&out).contains("fatal: not a git repository"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn warns_when_origin_is_unreachable() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["remote", "add", "origin", "/nonexistent/kraken-origin"]);

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stderr(&out).contains("could not compare tags with origin"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn dry_run_needs_a_level() {
    let repo = Repo::new();
    repo.node();

    assert_failed_with(&repo.kraken(&["--dry-run"]), "LEVEL");
}

#[test]
fn refuses_to_release_files_changed_after_the_start() {
    let repo = Repo::new();
    repo.node();
    let release = kraken::release::Release::prepare(repo.path()).unwrap();
    let plan = release.plan(kraken::version::Level::Patch).unwrap();
    repo.write("package.json", &PACKAGE_JSON.replace("demo", "changed"));

    let err = release.apply(&plan).unwrap_err().to_string();

    assert!(err.contains("uncommitted changes"), "{err}");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_to_release_after_a_branch_switch() {
    let repo = Repo::new();
    repo.node();
    let release = kraken::release::Release::prepare(repo.path()).unwrap();
    let plan = release.plan(kraken::version::Level::Patch).unwrap();
    repo.git(&["checkout", "-q", "-b", "other"]);

    let err = release.apply(&plan).unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_to_release_after_a_new_commit() {
    let repo = Repo::new();
    repo.node();
    let release = kraken::release::Release::prepare(repo.path()).unwrap();
    let plan = release.plan(kraken::version::Level::Patch).unwrap();
    repo.write("notes.md", "");
    repo.commit_all();
    let head = repo.head();

    let err = release.apply(&plan).unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert_eq!(repo.head(), head);
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}
