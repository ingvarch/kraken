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
const COMPOSER_JSON: &str = r#"{
    "name": "acme/widget",
    "description": "A widget",
    "version": "1.2.3",
    "type": "wordpress-plugin",
    "require": {
        "php": ">=8.1",
        "psr/log": "^3.0"
    },
    "require-dev": {},
    "autoload": { "psr-4": { "Acme\\Widget\\": "src/" } },
    "extra": { "author": "Jürgen", "branch-alias": { "dev-main": "1.x-dev" } },
    "config": { "sort-packages": true, "platform": { "php": "8.1" } }
}
"#;
const COMPOSER_LOCK: &str = r#"{
    "_readme": ["This file locks the dependencies."],
    "content-hash": "ba6abe21b53dc5723447b7e88299a7b4",
    "packages": [{ "name": "psr/log", "version": "3.0.0" }],
    "plugin-api-version": "2.6.0"
}
"#;
const COMPOSER_HASH_124: &str = "9ccd3108f58143b0fc888427a47fde1c";
const CARGO_TOML: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
const CARGO_LOCK: &str = "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n";
const TEMPLATE: &str = kraken::changelog::TEMPLATE;
const KRAKEN_LINE: &str =
    "<!-- kraken adds a section for every release. Delete this line to stop it. -->";
const RELEASED_AT: &str = "2026-11-04T12:00:00+0000";

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

fn shallow_clone(source: &Repo) -> (TempDir, std::path::PathBuf) {
    let holder = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.path().display());
    git_in(
        holder.path(),
        &["clone", "-q", "--depth", "1", &url, "repo"],
    );
    let path = holder.path().join("repo");
    (holder, path)
}

fn bare_origin() -> TempDir {
    let origin = tempfile::tempdir().unwrap();
    git_in(origin.path(), &["init", "-q", "--bare"]);
    origin
}

/// A GitHub that works offline: `bin/ssh` runs git's commands in a directory with the bare
/// repository `o/r.git`, and answers `ssh -G` with `FAKE_SSH_HOSTNAME` or the host asked for.
struct FakeGitHub {
    dir: TempDir,
}

impl FakeGitHub {
    fn new() -> FakeGitHub {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let script = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = -G ]; then echo \"hostname ${{FAKE_SSH_HOSTNAME:-$2}}\"; exit 0; fi\n\
             while [ $# -gt 0 ]; do case \"$1\" in -o|-p|-i|-l|-F) shift 2;; -*) shift;; *) break;; esac; done\n\
             shift\n\
             cd \"{}/remote\" && exec sh -c \"$*\"\n",
            dir.path().display()
        );
        fs::write(bin.join("ssh"), script).unwrap();
        fs::set_permissions(bin.join("ssh"), fs::Permissions::from_mode(0o755)).unwrap();
        let bare = dir.path().join("remote/o/r.git");
        fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "-q", "--bare"]);
        FakeGitHub { dir }
    }

    fn path(&self) -> String {
        format!(
            "{}:{}",
            self.dir.path().join("bin").display(),
            std::env::var("PATH").unwrap()
        )
    }
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

    fn commit(&self, message: &str) {
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
    }

    /// Runs kraken with the date the release commit gets fixed to [`RELEASED_AT`].
    fn kraken_dated(&self, args: &[&str]) -> Output {
        isolate(Command::new(env!("CARGO_BIN_EXE_kraken")).args(args))
            .env("GIT_COMMITTER_DATE", RELEASED_AT)
            .current_dir(self.path())
            .output()
            .unwrap()
    }

    /// A Node project at v1.2.3 with a committed CHANGELOG.md.
    fn node_with_changelog(&self) {
        self.write("CHANGELOG.md", TEMPLATE);
        self.node();
        self.git(&["tag", "v1.2.3"]);
    }

    fn add_github_origin(&self, _github: &FakeGitHub) {
        self.set_origin("git@github.com:o/r.git");
    }

    fn set_origin(&self, url: &str) {
        self.git(&["remote", "add", "origin", url]);
    }

    /// Runs kraken dated [`RELEASED_AT`], with the fake `ssh` of `github` first on PATH.
    fn kraken_on(&self, github: &FakeGitHub, args: &[&str], hostname: &str) -> Output {
        isolate(Command::new(env!("CARGO_BIN_EXE_kraken")).args(args))
            .env("GIT_COMMITTER_DATE", RELEASED_AT)
            .env("PATH", github.path())
            .env("FAKE_SSH_HOSTNAME", hostname)
            .current_dir(self.path())
            .output()
            .unwrap()
    }

    fn commit_at(&self, message: &str, date: &str) {
        check(isolate(
            Command::new("git")
                .args(["commit", "-q", "--allow-empty", "-m", message])
                .env("GIT_COMMITTER_DATE", date)
                .current_dir(self.path()),
        ));
    }

    /// An annotated tag made at `date`.
    fn tag_at(&self, tag: &str, date: &str) {
        check(isolate(
            Command::new("git")
                .args(["tag", "-a", tag, "-m", tag])
                .env("GIT_COMMITTER_DATE", date)
                .current_dir(self.path()),
        ));
    }

    fn light_tag(&self, tag: &str) {
        self.git(&["tag", tag]);
    }

    /// Three releases: v1.0.0 with `init`, v1.0.1 with only a `docs:` commit, v1.1.0 with a feature.
    fn three_releases(&self) {
        self.commit_at("init", "2026-09-01T10:00:00+0000");
        self.tag_at("v1.0.0", "2026-09-01T10:00:00+0000");
        self.commit_at("docs: explain it", "2026-09-10T10:00:00+0000");
        self.tag_at("v1.0.1", "2026-09-20T10:00:00+0000");
        self.commit_at("feat(cli): add a command", "2026-10-01T10:00:00+0000");
        self.tag_at("v1.1.0", "2026-10-01T10:00:00+0000");
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
fn version_file_patch_release() {
    let repo = Repo::new();
    repo.write("VERSION", "1.2.3\n");
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.read("VERSION"), "1.2.4\n");
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn deno_jsonc_patch_release_keeps_comments() {
    const DENO_JSONC: &str = "{\n  // published to JSR\n  \"name\": \"@demo/app\",\n  \"version\": \"1.2.3\", // bumped by kraken\n  \"exports\": \"./mod.ts\"\n}\n";
    let repo = Repo::new();
    repo.write("deno.jsonc", DENO_JSONC);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("deno.jsonc"),
        DENO_JSONC.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn gradle_properties_patch_release() {
    const GRADLE: &str = "# Gradle\norg.gradle.jvmargs=-Xmx2g\ngroup = com.example\nversion = 1.2.3\nversionCode=7\n";
    let repo = Repo::new();
    repo.write("gradle.properties", GRADLE);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("gradle.properties"),
        GRADLE.replace("version = 1.2.3", "version = 1.2.4")
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn xcode_patch_release_sets_every_configuration() {
    const PBXPROJ: &str = "{\n\t\t\t\tCURRENT_PROJECT_VERSION = 7;\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n\t\t\t\tPRODUCT_NAME = App;\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n}\n";
    let repo = Repo::new();
    repo.write("App.xcodeproj/project.pbxproj", PBXPROJ);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("App.xcodeproj/project.pbxproj"),
        PBXPROJ.replace("MARKETING_VERSION = 1.2.3", "MARKETING_VERSION = 1.2.4")
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn pubspec_patch_release_keeps_the_build_number() {
    const PUBSPEC: &str = "name: app\nversion: 1.2.3+7 # store\n";
    const LOCK: &str = "packages:\n  meta:\n    dependency: transitive\n    description: {name: meta}\n    source: hosted\n    version: \"1.2.3\"\n";
    let repo = Repo::new();
    repo.write("pubspec.yaml", PUBSPEC);
    repo.write("pubspec.lock", LOCK);
    repo.commit_all();
    repo.git(&["tag", "v1.2.3"]);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("pubspec.yaml"),
        "name: app\nversion: 1.2.4+7 # store\n"
    );
    assert_eq!(repo.read("pubspec.lock"), LOCK);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn pubspec_without_a_version_releases_from_tags() {
    const PUBSPEC: &str = "name: _\npublish_to: none\n";
    let repo = Repo::new();
    repo.write("pubspec.yaml", PUBSPEC);
    repo.commit_all();
    repo.git(&["tag", "v0.4.0"]);
    let before = repo.head();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.head(), before);
    assert_eq!(repo.tag_commit("v0.4.1"), before);
    assert_eq!(repo.read("pubspec.yaml"), PUBSPEC);
}

#[test]
fn pubspec_pre_release_releases_the_final_version() {
    const PUBSPEC: &str = "name: app\nversion: 1.2.3-dev.1+4\n";
    let repo = Repo::new();
    repo.write("pubspec.yaml", PUBSPEC);
    repo.commit_all();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.read("pubspec.yaml"), "name: app\nversion: 1.2.3+4\n");
    assert_eq!(repo.tags(), ["v1.2.3"]);
}

const PROJECT_YML: &str = "name: App\nsettings:\n  base:\n    MARKETING_VERSION: \"1.2.3\" # app\n    CURRENT_PROJECT_VERSION: 7\n";
const GENERATED_PBXPROJ: &str =
    "{\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n}\n";

#[test]
fn xcodegen_release_with_a_tracked_project_commits_both_files() {
    let repo = Repo::new();
    repo.write("project.yml", PROJECT_YML);
    repo.write("App.xcodeproj/project.pbxproj", GENERATED_PBXPROJ);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("project.yml"),
        PROJECT_YML.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.read("App.xcodeproj/project.pbxproj"),
        GENERATED_PBXPROJ.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "App.xcodeproj/project.pbxproj\nproject.yml"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn xcodegen_release_with_a_non_ascii_project_name_commits_both_files() {
    let repo = Repo::new();
    repo.write(
        "project.yml",
        &PROJECT_YML.replace("name: App", "name: Caf\u{e9}"),
    );
    repo.write("Caf\u{e9}.xcodeproj/project.pbxproj", GENERATED_PBXPROJ);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.git(&[
            "-c",
            "core.quotePath=false",
            "show",
            "--name-only",
            "--format=",
            "HEAD"
        ]),
        "Caf\u{e9}.xcodeproj/project.pbxproj\nproject.yml"
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn xcodegen_release_updates_an_ignored_project_and_leaves_it_out() {
    let repo = Repo::new();
    repo.write(".gitignore", "*.xcodeproj\n");
    repo.write("project.yml", PROJECT_YML);
    repo.commit_all();
    repo.write("App.xcodeproj/project.pbxproj", GENERATED_PBXPROJ);

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("App.xcodeproj/project.pbxproj"),
        GENERATED_PBXPROJ.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "project.yml"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
}

#[test]
fn xcodegen_release_leaves_an_unrelated_project_alone() {
    const EXAMPLE: &str = "{\n\t\t\t\tMARKETING_VERSION = 0.1.0;\n}\n";
    let repo = Repo::new();
    repo.write(".gitignore", "App.xcodeproj\n");
    repo.write("project.yml", PROJECT_YML);
    repo.write("Example.xcodeproj/project.pbxproj", EXAMPLE);
    repo.commit_all();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.read("Example.xcodeproj/project.pbxproj"), EXAMPLE);
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "project.yml"
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn xcode_release_with_a_non_ascii_project_name() {
    const PBXPROJ: &str = "{\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n}\n";
    let repo = Repo::new();
    repo.write("Caf\u{e9}.xcodeproj/project.pbxproj", PBXPROJ);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("Caf\u{e9}.xcodeproj/project.pbxproj"),
        PBXPROJ.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&[
            "-c",
            "core.quotePath=false",
            "show",
            "--name-only",
            "--format=",
            "HEAD"
        ]),
        "Caf\u{e9}.xcodeproj/project.pbxproj"
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn xcode_project_that_git_ignores_is_left_alone() {
    const PBXPROJ: &str = "{\n\t\t\t\tMARKETING_VERSION = 1.0;\n}\n";
    let repo = Repo::new();
    repo.write(".gitignore", "*.xcodeproj\n");
    repo.write("App.xcodeproj/project.pbxproj", PBXPROJ);
    repo.commit_all();
    repo.git(&["tag", "v1.2.3"]);
    let head = repo.head();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.head(), head);
    assert_eq!(repo.tag_commit("v1.2.4"), head);
    assert_eq!(repo.read("App.xcodeproj/project.pbxproj"), PBXPROJ);
}

#[test]
fn xcode_projects_that_git_ignores_do_not_count_as_several() {
    const PBXPROJ: &str = "{\n\t\t\t\tMARKETING_VERSION = 1.0;\n}\n";
    let repo = Repo::new();
    repo.write(".gitignore", "*.xcodeproj\n");
    repo.write("App.xcodeproj/project.pbxproj", PBXPROJ);
    repo.write("Tool.xcodeproj/project.pbxproj", PBXPROJ);
    repo.commit_all();
    repo.git(&["tag", "v1.2.3"]);
    let head = repo.head();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.head(), head);
    assert_eq!(repo.tag_commit("v1.2.4"), head);
}

#[test]
fn xcode_projects_without_a_version_leave_a_node_release_alone() {
    let repo = Repo::new();
    for name in ["A", "B"] {
        repo.write(
            &format!("{name}.xcodeproj/project.pbxproj"),
            "{\n\t\t\t\tPRODUCT_NAME = App;\n}\n",
        );
    }
    repo.node();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.2.4")
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn symlinked_xcode_project_leaves_a_node_release_alone() {
    let repo = Repo::new();
    repo.write(
        "real/App.xcodeproj/project.pbxproj",
        "{\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n}\n",
    );
    std::os::unix::fs::symlink("real/App.xcodeproj", repo.path().join("App.xcodeproj")).unwrap();
    repo.node();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.2.4")
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn tracked_xcode_project_counts_despite_an_ignore_pattern() {
    const PBXPROJ: &str = "{\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n}\n";
    let repo = Repo::new();
    repo.write(".gitignore", "*.xcodeproj\n");
    repo.write("App.xcodeproj/project.pbxproj", PBXPROJ);
    repo.git(&["add", "-f", "App.xcodeproj/project.pbxproj"]);
    repo.commit_all();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(
        repo.read("App.xcodeproj/project.pbxproj"),
        PBXPROJ.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
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
fn dry_run_without_a_changelog_needs_no_committer_identity() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    repo.commit("fix: c");

    let out = isolate(Command::new(env!("CARGO_BIN_EXE_kraken")).args(["patch", "--dry-run"]))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "user.useConfigOnly")
        .env("GIT_CONFIG_VALUE_0", "true")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert_ok(&out);
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
    let (_holder, clone) = shallow_clone(&source);

    assert_failed_with(&kraken_in(&clone, &["patch", "--yes"]), "shallow");
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
    let release = kraken::release::Release::prepare(repo.path(), true).unwrap();
    let plan = release.plan(kraken::version::Level::Patch, None).unwrap();
    repo.write("package.json", &PACKAGE_JSON.replace("demo", "changed"));

    let err = release.apply(&plan).unwrap_err().to_string();

    assert!(err.contains("uncommitted changes"), "{err}");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_to_release_after_a_branch_switch() {
    let repo = Repo::new();
    repo.node();
    let release = kraken::release::Release::prepare(repo.path(), true).unwrap();
    let plan = release.plan(kraken::version::Level::Patch, None).unwrap();
    repo.git(&["checkout", "-q", "-b", "other"]);

    let err = release.apply(&plan).unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert!(repo.tags().is_empty());
}

#[test]
fn refuses_to_release_after_a_new_commit() {
    let repo = Repo::new();
    repo.node();
    let release = kraken::release::Release::prepare(repo.path(), true).unwrap();
    let plan = release.plan(kraken::version::Level::Patch, None).unwrap();
    repo.write("notes.md", "");
    repo.commit_all();
    let head = repo.head();

    let err = release.apply(&plan).unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert_eq!(repo.head(), head);
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}

#[test]
fn version_prints_the_version_from_version_files() {
    let repo = Repo::new();
    repo.node();

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.2.3\n");
    assert_eq!(stderr(&out), "");
}

#[test]
fn version_prints_the_latest_tag_without_version_files() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();
    repo.git(&["tag", "v1.2.3"]);

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.2.3\n");
}

#[test]
fn version_is_zero_without_version_files_or_tags() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "0.0.0\n");
}

#[test]
fn version_allows_uncommitted_changes() {
    let repo = Repo::new();
    repo.node();
    repo.write("package.json", &PACKAGE_JSON.replace("1.2.3", "1.5.0"));

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.5.0\n");
}

#[test]
fn version_allows_a_detached_head() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["checkout", "-q", "--detach"]);

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.2.3\n");
}

#[test]
fn version_warns_on_stderr_when_latest_tag_differs() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.0.0"]);

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.2.3\n");
    assert_eq!(
        stderr(&out),
        "warning: latest tag v1.0.0 differs from version 1.2.3 in the version files\n"
    );
}

#[test]
fn version_refuses_disagreeing_version_files() {
    let repo = Repo::new();
    repo.write("package.json", PACKAGE_JSON);
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"1.3.0\"\n",
    );
    repo.commit_all();

    let out = repo.kraken(&["version"]);

    assert_failed_with(&out, "version files disagree");
    assert_eq!(stdout(&out), "");
}

#[test]
fn version_makes_no_commit_and_no_tag() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.0.0"]);
    let head = repo.head();
    let tags = repo.tags();

    assert_ok(&repo.kraken(&["version"]));

    assert_eq!(repo.head(), head);
    assert_eq!(repo.tags(), tags);
    assert_eq!(repo.status(), "");
}

#[test]
fn version_does_not_ask_origin() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["remote", "add", "origin", "/nonexistent/origin"]);

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stderr(&out), "");
}

#[test]
fn version_refuses_a_shallow_clone_that_needs_tags() {
    let source = Repo::new();
    source.write("main.go", "package main\n");
    source.commit_all();
    source.git(&["tag", "v1.0.0"]);
    source.write("a.txt", "");
    source.commit_all();
    let (_holder, clone) = shallow_clone(&source);

    assert_failed_with(&kraken_in(&clone, &["version"]), "shallow");
}

#[test]
fn version_works_in_a_shallow_clone_with_version_files() {
    let source = Repo::new();
    source.node();
    source.write("a.txt", "");
    source.commit_all();
    let (_holder, clone) = shallow_clone(&source);

    let out = kraken_in(&clone, &["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.2.3\n");
}

#[test]
fn version_refuses_repository_without_commits() {
    let repo = Repo::new();
    repo.write("package.json", PACKAGE_JSON);

    assert_failed_with(&repo.kraken(&["version"]), "no commits");
}

#[test]
fn version_refuses_outside_git() {
    let dir = tempfile::tempdir().unwrap();

    assert_failed_with(
        &kraken_in(dir.path(), &["version"]),
        "not inside a git repository",
    );
}

#[test]
fn help_is_not_a_subcommand() {
    let repo = Repo::new();
    repo.node();

    assert_failed_with(&repo.kraken(&["help"]), "invalid value 'help'");
}

#[test]
fn version_takes_no_flags() {
    let repo = Repo::new();
    repo.node();

    for flag in ["--yes", "--dry-run"] {
        let out = repo.kraken(&["version", flag]);
        assert_eq!(out.status.code(), Some(2), "{flag}");
        assert!(
            stderr(&out).contains(&format!("unexpected argument '{flag}' found")),
            "{flag}: {}",
            stderr(&out)
        );
    }
}

fn cargo_tagged(repo: &Repo) {
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("Cargo.lock", CARGO_LOCK);
    repo.write("src/lib.rs", "");
    repo.commit_all();
    repo.git(&["tag", "v0.1.0"]);
}

fn go_module(repo: &Repo) {
    repo.write("go.mod", "module demo\n");
    repo.commit_all();
}

#[test]
fn version_next_prints_the_next_version() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    repo.commit("fix: b");

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.3.0\n");
    assert_eq!(
        stderr(&out),
        "Changes: 1 feature, 1 fix since v1.2.3\nLevel: minor\n"
    );
}

#[test]
fn version_next_prints_major_for_a_breaking_change() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat!: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "2.0.0\n");
    assert_eq!(
        stderr(&out),
        "Changes: 1 breaking change since v1.2.3\nLevel: major\n"
    );
}

#[test]
fn version_next_prints_minor_for_a_breaking_change_in_0x() {
    let repo = Repo::new();
    cargo_tagged(&repo);
    repo.commit("feat!: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "0.2.0\n");
    assert!(
        stderr(&out).contains("Level: minor (breaking changes bump minor in 0.x)\n"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn version_next_counts_every_commit_without_tags() {
    let repo = Repo::new();
    go_module(&repo);
    repo.commit("fix: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "0.0.1\n");
    assert_eq!(
        stderr(&out),
        "Changes: 1 fix, 1 other commit since the first commit\nLevel: patch\n"
    );
}

#[test]
fn version_next_starts_from_the_latest_tag_without_version_files() {
    let repo = Repo::new();
    go_module(&repo);
    repo.git(&["tag", "v1.0.0"]);
    repo.commit("feat: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.1.0\n");
}

#[test]
fn version_next_refuses_without_new_commits() {
    let repo = Repo::new();
    node_tagged(&repo);

    let out = repo.kraken(&["version", "--next"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "error: no commits since v1.2.3; nothing to release\n"
    );
}

#[test]
fn version_next_warns_when_latest_tag_differs() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.0.0"]);
    repo.commit("feat: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.3.0\n");
    assert_eq!(
        stderr(&out),
        "warning: latest tag v1.0.0 differs from version 1.2.3 in the version files\n\
         Changes: 1 feature since v1.0.0\nLevel: minor\n"
    );
}

#[test]
fn version_next_allows_uncommitted_changes() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    repo.write("package.json", &PACKAGE_JSON.replace("1.2.3", "1.5.0"));

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.6.0\n");
}

#[test]
fn version_next_allows_a_detached_head() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    repo.git(&["checkout", "-q", "--detach"]);

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.3.0\n");
}

#[test]
fn version_next_refuses_a_shallow_clone_with_version_files() {
    let source = Repo::new();
    node_tagged(&source);
    source.commit("feat: a");
    let (_holder, clone) = shallow_clone(&source);

    let out = kraken_in(&clone, &["version", "--next"]);

    assert_failed_with(&out, "shallow");
    assert_eq!(stdout(&out), "");
}

#[test]
fn version_next_does_not_ask_origin() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("fix: a");
    repo.git(&["remote", "add", "origin", "/nonexistent/origin"]);

    let out = repo.kraken(&["version", "--next"]);

    assert_ok(&out);
    assert!(!stderr(&out).contains("origin"), "{}", stderr(&out));
}

#[test]
fn version_next_refuses_an_existing_tag() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.commit("chore: x");
    repo.git(&["tag", "v1.3.0"]);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("feat: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "error: tag v1.3.0 already exists\n");
}

#[test]
fn version_next_refuses_a_version_not_above_the_latest_tag() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.5.0"]);
    repo.commit("fix: a");

    let out = repo.kraken(&["version", "--next"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "error: 1.2.4 is not higher than the latest tag v1.5.0; bring the version files up to date first\n"
    );
}

#[test]
fn version_next_makes_no_commit_and_no_tag() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    let head = repo.head();
    let tags = repo.tags();

    assert_ok(&repo.kraken(&["version", "--next"]));

    assert_eq!(repo.head(), head);
    assert_eq!(repo.tags(), tags);
    assert_eq!(repo.status(), "");
}

#[test]
fn version_next_takes_no_release_flags() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");

    for flag in ["--yes", "--dry-run"] {
        let out = repo.kraken(&["version", "--next", flag]);
        assert_eq!(out.status.code(), Some(2), "{flag}");
        assert!(
            stderr(&out).contains(&format!("unexpected argument '{flag}' found")),
            "{flag}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn version_next_agrees_with_auto() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");

    let out = repo.kraken(&["version", "--next"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "1.3.0\n");
    let next = stdout(&out).trim().to_string();
    let dry_run = stdout(&repo.kraken(&["auto", "--dry-run"]));

    assert!(
        dry_run.contains(&format!("  tag v{next}")),
        "{next}{dry_run}"
    );
}

#[test]
fn dash_dash_version_still_prints_kraken_version() {
    let repo = Repo::new();

    let out = repo.kraken(&["--version"]);

    assert_ok(&out);
    assert_eq!(
        stdout(&out),
        format!("kraken {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn changelog_creates_the_file() {
    let repo = Repo::new();
    repo.node();

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert!(
        stdout(&out).contains("Created CHANGELOG.md"),
        "{}",
        stdout(&out)
    );
    assert_eq!(repo.status(), "?? CHANGELOG.md");
}

#[test]
fn changelog_goes_to_the_repository_root() {
    let repo = Repo::new();
    repo.write("src/index.js", "");
    repo.node();

    assert_ok(&kraken_in(&repo.path().join("src"), &["changelog"]));

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
}

#[test]
fn changelog_puts_the_kraken_line_in_the_new_file() {
    let repo = Repo::new();
    repo.node();

    assert_ok(&repo.kraken(&["changelog"]));

    assert!(
        repo.read("CHANGELOG.md")
            .lines()
            .any(|line| line == KRAKEN_LINE),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn changelog_without_the_kraken_line_tells_what_to_add() {
    let repo = Repo::new();
    repo.node();
    repo.write("CHANGELOG.md", "mine\n");

    let out = repo.kraken(&["changelog"]);

    assert_failed_with(
        &out,
        "CHANGELOG.md already exists. To have every release add a section to it, add this line on a line of its own, not indented:",
    );
    assert!(stderr(&out).contains(KRAKEN_LINE), "{}", stderr(&out));
    assert_eq!(repo.read("CHANGELOG.md"), "mine\n");
}

#[test]
fn changelog_with_the_kraken_line_is_ready() {
    let repo = Repo::new();
    repo.node_with_changelog();

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains(
            "CHANGELOG.md has the kraken line. Once it is committed, every release adds a section to it."
        ),
        "{}",
        stdout(&out)
    );
    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
}

#[test]
fn changelog_refuses_a_file_that_is_not_utf8() {
    let repo = Repo::new();
    repo.node();
    fs::write(
        repo.path().join("CHANGELOG.md"),
        [TEMPLATE.as_bytes(), b"- J\xf6rg\n"].concat(),
    )
    .unwrap();

    assert_failed_with(&repo.kraken(&["changelog"]), "CHANGELOG.md is not UTF-8");
}

#[test]
fn changelog_refuses_a_symlink() {
    let repo = Repo::new();
    repo.write("docs/changelog.md", TEMPLATE);
    std::os::unix::fs::symlink("docs/changelog.md", repo.path().join("CHANGELOG.md")).unwrap();
    repo.node();

    assert_failed_with(
        &repo.kraken(&["changelog"]),
        "kraken writes only to a regular file named exactly CHANGELOG.md",
    );
    assert_eq!(repo.read("docs/changelog.md"), TEMPLATE);
}

#[test]
fn changelog_refuses_outside_git() {
    let dir = tempfile::tempdir().unwrap();

    assert_failed_with(
        &kraken_in(dir.path(), &["changelog"]),
        "not inside a git repository",
    );

    assert!(!dir.path().join("CHANGELOG.md").exists());
}

#[test]
fn changelog_takes_no_flags() {
    let repo = Repo::new();
    repo.node();

    for flag in ["--yes", "--dry-run", "--no-changelog"] {
        let out = repo.kraken(&["changelog", flag]);
        assert_eq!(out.status.code(), Some(2), "{flag}");
        assert!(
            stderr(&out).contains(&format!("unexpected argument '{flag}' found")),
            "{flag}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn release_adds_the_commits_since_the_latest_tag_to_the_changelog() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat(cli): add a command");
    repo.commit("fix: handle a crash");
    repo.commit("docs: explain it");

    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{TEMPLATE}\n## [1.2.4] - 2026-11-04\n\n### Added\n\n- **cli**: add a command\n\n### Fixed\n\n- handle a crash\n"
        )
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "CHANGELOG.md\npackage-lock.json\npackage.json"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn changelog_section_leaves_out_merge_commits() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.git(&["checkout", "-q", "-b", "topic"]);
    repo.commit("fix: on a branch");
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge branch 'topic'",
        "topic",
    ]);

    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));

    let changelog = repo.read("CHANGELOG.md");
    assert!(changelog.contains("- on a branch"), "{changelog}");
    assert!(!changelog.contains("Merge"), "{changelog}");
}

#[test]
fn first_release_puts_every_commit_in_the_changelog() {
    let repo = Repo::new();
    repo.write("CHANGELOG.md", TEMPLATE);
    repo.node();

    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!("{TEMPLATE}\n## [1.2.4] - 2026-11-04\n\n### Changed\n\n- init\n")
    );
}

#[test]
fn tag_only_release_commits_the_changelog() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.write("CHANGELOG.md", TEMPLATE);
    repo.commit_all();
    repo.git(&["tag", "1.0.0"]);
    repo.commit("feat: serve over https");
    let head = repo.head();

    assert_ok(&repo.kraken_dated(&["minor", "--yes"]));

    assert_eq!(repo.git(&["rev-parse", "HEAD~1"]), head);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): 1.1.0"
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "CHANGELOG.md"
    );
    assert!(
        repo.read("CHANGELOG.md")
            .contains("## [1.1.0] - 2026-11-04")
    );
    assert_eq!(repo.tag_commit("1.1.0"), repo.head());
}

#[test]
fn changelog_reads_the_body_of_a_commit() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "feat: new config format",
        "-m",
        "BREAKING CHANGE: the old format is gone",
    ]);

    assert_ok(&repo.kraken_dated(&["major", "--yes"]));

    assert!(
        repo.read("CHANGELOG.md")
            .contains("## [2.0.0] - 2026-11-04\n\n### Breaking\n\n- new config format\n"),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn changelog_ignores_log_show_signature() {
    let repo = Repo::new();
    repo.node_with_changelog();
    let key = repo.path().join(".git/signing-key");
    check(
        Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", ""])
            .arg("-f")
            .arg(&key),
    );
    let public = fs::read_to_string(key.with_extension("pub")).unwrap();
    let signers = repo.path().join(".git/allowed-signers");
    fs::write(&signers, format!("test@example.com {public}")).unwrap();
    repo.git(&["config", "gpg.format", "ssh"]);
    repo.git(&["config", "user.signingkey", key.to_str().unwrap()]);
    repo.git(&[
        "config",
        "gpg.ssh.allowedSignersFile",
        signers.to_str().unwrap(),
    ]);
    repo.git(&["config", "log.showSignature", "true"]);
    repo.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "-S",
        "-m",
        "feat: add a signed feature",
    ]);

    let out = repo.kraken_dated(&["minor", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("    ### Added\n\n    - add a signed feature\n"),
        "{}",
        stdout(&out)
    );
    assert!(!stdout(&out).contains("signature"), "{}", stdout(&out));
}

#[test]
fn release_leaves_a_changelog_without_the_kraken_line_alone() {
    let by_hand = "# Changelog\n\n## [1.2.3] - 2026-10-01\n\n- by hand\n";
    let repo = Repo::new();
    repo.write("CHANGELOG.md", by_hand);
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    repo.commit("feat: add a command");

    let dry_run = repo.kraken(&["patch", "--dry-run"]);
    assert_ok(&dry_run);
    assert!(
        !stdout(&dry_run).contains("CHANGELOG.md"),
        "{}",
        stdout(&dry_run)
    );

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.read("CHANGELOG.md"), by_hand);
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "package-lock.json\npackage.json"
    );
}

#[test]
fn release_writes_to_your_own_changelog_with_the_kraken_line() {
    let repo = Repo::new();
    repo.write(
        "CHANGELOG.md",
        &format!(
            "# Changes\n\n## [Unreleased]\n\n- by hand\n\n{KRAKEN_LINE}\n\n## [1.2.3] - 2026-10-01\n"
        ),
    );
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    repo.commit("fix: handle a crash");

    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "# Changes\n\n## [Unreleased]\n\n- by hand\n\n{KRAKEN_LINE}\n\n## [1.2.4] - 2026-11-04\n\n### Fixed\n\n- handle a crash\n\n## [1.2.3] - 2026-10-01\n"
        )
    );
}

#[test]
fn release_refuses_a_changelog_that_is_not_utf8() {
    let repo = Repo::new();
    fs::write(
        repo.path().join("CHANGELOG.md"),
        [TEMPLATE.as_bytes(), b"- J\xf6rg\n"].concat(),
    )
    .unwrap();
    repo.node();

    assert_failed_with(
        &repo.kraken(&["patch", "--yes"]),
        "CHANGELOG.md is not UTF-8",
    );
    assert!(repo.tags().is_empty());
}

#[test]
fn release_ignores_a_changelog_without_the_kraken_line_that_is_not_utf8() {
    let repo = Repo::new();
    fs::write(repo.path().join("CHANGELOG.md"), b"- J\xf6rg\n").unwrap();
    repo.node();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(
        fs::read(repo.path().join("CHANGELOG.md")).unwrap(),
        b"- J\xf6rg\n"
    );
}

#[test]
fn untracked_changelog_without_the_kraken_line_does_not_stop_a_release() {
    let repo = Repo::new();
    repo.node();
    repo.write("CHANGELOG.md", "mine\n");

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.read("CHANGELOG.md"), "mine\n");
    assert_eq!(repo.status(), "?? CHANGELOG.md");
}

#[test]
fn no_changelog_releases_without_the_changelog() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");

    let dry_run = repo.kraken(&["patch", "--dry-run", "--no-changelog"]);
    assert_ok(&dry_run);
    assert!(
        !stdout(&dry_run).contains("CHANGELOG.md"),
        "{}",
        stdout(&dry_run)
    );

    assert_ok(&repo.kraken(&["patch", "--yes", "--no-changelog"]));

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "package-lock.json\npackage.json"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn no_changelog_releases_past_an_untracked_changelog() {
    let repo = Repo::new();
    repo.node();
    repo.write("CHANGELOG.md", TEMPLATE);

    assert_ok(&repo.kraken(&["patch", "--yes", "--no-changelog"]));

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert_eq!(repo.status(), "?? CHANGELOG.md");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn no_changelog_works_without_a_level() {
    let repo = Repo::new();
    repo.node();

    assert_failed_with(
        &repo.kraken(&["--no-changelog"]),
        "choosing the level needs stdin and stdout to be a terminal",
    );
}

#[test]
fn changelog_must_be_named_exactly() {
    let repo = Repo::new();
    repo.write("changelog.md", TEMPLATE);
    repo.node();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.read("changelog.md"), TEMPLATE);
    assert_eq!(repo.status(), "");
}

#[test]
fn symlinked_changelog_is_left_alone() {
    let repo = Repo::new();
    repo.write("docs/changelog.md", TEMPLATE);
    std::os::unix::fs::symlink("docs/changelog.md", repo.path().join("CHANGELOG.md")).unwrap();
    repo.node();

    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_eq!(repo.read("docs/changelog.md"), TEMPLATE);
    assert_eq!(repo.status(), "");
}

#[test]
fn refuses_untracked_changelog() {
    let repo = Repo::new();
    repo.node();
    repo.write("CHANGELOG.md", TEMPLATE);

    assert_failed_with(
        &repo.kraken(&["patch", "--yes"]),
        "CHANGELOG.md is not tracked by git; commit it first",
    );
    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert!(repo.tags().is_empty());
}

#[test]
fn dry_run_shows_the_changelog_section_and_changes_nothing() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");

    let out = repo.kraken_dated(&["minor", "--dry-run"]);

    assert_ok(&out);
    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert!(repo.tags().contains(&"v1.2.3".to_string()) && repo.tags().len() == 1);
    assert!(
        stdout(&out).contains(
            "  add to CHANGELOG.md:\n\n    ## [1.3.0] - 2026-11-04\n\n    ### Added\n\n    - add a command\n"
        ),
        "{}",
        stdout(&out)
    );
}

#[test]
fn restores_the_changelog_when_commit_fails() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");
    let hook = repo.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(!repo.kraken(&["patch", "--yes"]).status.success());

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert_eq!(repo.status(), "");
}

const GH: &str = "https://github.com/o/r";

/// A repository at v1.2.3 with GitHub as origin and `changelog` committed.
fn github_repo(changelog: &str) -> (Repo, FakeGitHub) {
    let repo = Repo::new();
    repo.write("CHANGELOG.md", changelog);
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    repo.commit("feat: add a command");
    let github = FakeGitHub::new();
    repo.add_github_origin(&github);
    (repo, github)
}

fn github_release(repo: &Repo, github: &FakeGitHub) {
    let out = repo.kraken_on(github, &["patch", "--yes"], "github.com");
    assert_ok(&out);
}

#[test]
fn release_links_the_section_to_the_github_compare_page() {
    let (repo, github) = github_repo(TEMPLATE);

    github_release(&repo, &github);

    let changelog = repo.read("CHANGELOG.md");
    assert!(
        changelog.ends_with(&format!("\n[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n")),
        "{changelog}"
    );
    assert!(
        repo.git(&["show", "HEAD:CHANGELOG.md"])
            .contains("[1.2.4]: ")
    );
}

#[test]
fn first_release_links_the_tag_page() {
    let repo = Repo::new();
    repo.write("CHANGELOG.md", TEMPLATE);
    repo.write("VERSION", "0.1.0\n");
    repo.commit_all();
    let github = FakeGitHub::new();
    repo.add_github_origin(&github);

    github_release(&repo, &github);

    assert!(
        repo.read("CHANGELOG.md")
            .ends_with(&format!("\n[0.1.1]: {GH}/releases/tag/v0.1.1\n"))
    );
}

#[test]
fn release_links_keep_the_tag_style() {
    let (repo, github) = github_repo(TEMPLATE);
    repo.git(&["tag", "-d", "v1.2.3"]);
    repo.git(&["tag", "1.2.3", "HEAD~1"]);

    github_release(&repo, &github);

    assert!(
        repo.read("CHANGELOG.md")
            .ends_with(&format!("[1.2.4]: {GH}/compare/1.2.3...1.2.4\n"))
    );
}

#[test]
fn release_moves_the_unreleased_link() {
    let changelog = format!(
        "{TEMPLATE}\n## [1.2.3] - 2026-10-01\n\n- a\n\n[Unreleased]: {GH}/compare/v1.2.3...HEAD\n[1.2.3]: {GH}/releases/tag/v1.2.3\n"
    );
    let (repo, github) = github_repo(&changelog);

    github_release(&repo, &github);

    assert!(
        repo.read("CHANGELOG.md").ends_with(&format!(
            "[Unreleased]: {GH}/compare/v1.2.4...HEAD\n[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n[1.2.3]: {GH}/releases/tag/v1.2.3\n"
        )),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn release_keeps_adding_links_to_your_own_changelog() {
    let changelog = format!(
        "# Changelog\n\n{KRAKEN_LINE}\n\n## [Unreleased]\n\n## [1.2.3] - 2026-10-01\n\n- a\n\n[unreleased]: {GH}/compare/v1.2.3...HEAD\n[1.2.3]: {GH}/compare/v1.2.2...v1.2.3\n"
    );
    let (repo, github) = github_repo(&changelog);

    github_release(&repo, &github);

    let text = repo.read("CHANGELOG.md");
    assert!(
        text.contains("## [Unreleased]\n\n## [1.2.4] - 2026-11-04\n"),
        "{text}"
    );
    assert!(
        text.ends_with(&format!(
            "[unreleased]: {GH}/compare/v1.2.4...HEAD\n[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n[1.2.3]: {GH}/compare/v1.2.2...v1.2.3\n"
        )),
        "{text}"
    );
}

#[test]
fn release_adds_no_links_to_a_changelog_with_releases_and_no_links() {
    let changelog = format!("{TEMPLATE}\n## [1.2.3] - 2026-10-01\n\n- a\n");
    let (repo, github) = github_repo(&changelog);

    github_release(&repo, &github);

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{TEMPLATE}\n## [1.2.4] - 2026-11-04\n\n### Added\n\n- add a command\n\n## [1.2.3] - 2026-10-01\n\n- a\n"
        )
    );
}

#[test]
fn release_adds_no_links_without_origin() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");

    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));

    assert!(!repo.read("CHANGELOG.md").contains("]: "));
}

#[test]
fn release_adds_no_links_for_another_host() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");
    let origin = bare_origin();
    repo.add_origin(&origin);

    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));

    assert!(!repo.read("CHANGELOG.md").contains("]: "));
}

#[test]
fn release_resolves_an_ssh_host_alias() {
    let (repo, github) = github_repo(TEMPLATE);
    repo.git(&[
        "remote",
        "set-url",
        "--push",
        "origin",
        "git@github-work:o/r.git",
    ]);

    assert_ok(&repo.kraken_on(&github, &["patch", "--yes"], "github.com"));

    assert!(
        repo.read("CHANGELOG.md")
            .ends_with(&format!("[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n"))
    );
}

#[test]
fn release_maps_the_github_port_443_host_back_to_github() {
    let (repo, github) = github_repo(TEMPLATE);
    repo.git(&[
        "remote",
        "set-url",
        "--push",
        "origin",
        "git@github.com:o/r.git",
    ]);

    assert_ok(&repo.kraken_on(&github, &["patch", "--yes"], "ssh.github.com"));

    assert!(
        repo.read("CHANGELOG.md")
            .ends_with(&format!("[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n"))
    );
}

#[test]
fn an_ssh_alias_for_another_host_gets_no_links() {
    let (repo, github) = github_repo(TEMPLATE);
    repo.git(&["remote", "set-url", "--push", "origin", "git@work:o/r.git"]);

    assert_ok(&repo.kraken_on(&github, &["patch", "--yes"], "git.example.com"));

    assert!(!repo.read("CHANGELOG.md").contains("]: "));
}

#[test]
fn origin_credentials_never_reach_the_output() {
    let (repo, github) = github_repo(TEMPLATE);
    repo.git(&[
        "remote",
        "set-url",
        "--push",
        "origin",
        "https://user:s3cret@github.com/o/r.git",
    ]);

    let out = repo.kraken_on(&github, &["patch", "--dry-run"], "github.com");

    assert_ok(&out);
    assert!(
        stdout(&out).contains(&format!("[1.2.4]: {GH}/compare/v1.2.3...v1.2.4")),
        "{}",
        stdout(&out)
    );
    assert!(!format!("{}{}", stdout(&out), stderr(&out)).contains("s3cret"));
}

#[test]
fn dry_run_shows_the_links() {
    let changelog = format!(
        "{TEMPLATE}\n## [1.2.3] - 2026-10-01\n\n- a\n\n[Unreleased]: {GH}/compare/v1.2.3...HEAD\n[1.2.3]: {GH}/releases/tag/v1.2.3\n"
    );
    let (repo, github) = github_repo(&changelog);

    let out = repo.kraken_on(&github, &["minor", "--dry-run"], "github.com");

    assert_ok(&out);
    assert!(
        stdout(&out).contains(&format!(
            "    - add a command\n\n  set the links at the end of CHANGELOG.md:\n\n    [Unreleased]: {GH}/compare/v1.3.0...HEAD\n    [1.3.0]: {GH}/compare/v1.2.3...v1.3.0\n"
        )),
        "{}",
        stdout(&out)
    );
    assert_eq!(repo.read("CHANGELOG.md"), changelog);
}

#[test]
fn restores_the_links_when_commit_fails() {
    let changelog = format!(
        "{TEMPLATE}\n## [1.2.3] - 2026-10-01\n\n- a\n\n[Unreleased]: {GH}/compare/v1.2.3...HEAD\n[1.2.3]: {GH}/releases/tag/v1.2.3\n"
    );
    let (repo, github) = github_repo(&changelog);
    let hook = repo.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        !repo
            .kraken_on(&github, &["patch", "--yes"], "github.com")
            .status
            .success()
    );

    assert_eq!(repo.read("CHANGELOG.md"), changelog);
    assert_eq!(repo.status(), "");
}

#[test]
fn a_changelog_without_releases_gets_the_section_above_its_links() {
    let changelog = format!("{TEMPLATE}\n[Unreleased]: {GH}/compare/v1.2.3...HEAD\n");
    let (repo, github) = github_repo(&changelog);

    github_release(&repo, &github);

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{TEMPLATE}\n## [1.2.4] - 2026-11-04\n\n### Added\n\n- add a command\n\n[Unreleased]: {GH}/compare/v1.2.4...HEAD\n[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n"
        )
    );
}

#[test]
fn final_release_links_from_the_last_final_tag() {
    let (repo, github) = github_repo(TEMPLATE);
    assert_ok(&repo.kraken_on(&github, &["minor", "--pre", "rc", "--yes"], "github.com"));
    repo.commit("feat: add another command");

    assert_ok(&repo.kraken_on(&github, &["minor", "--yes"], "github.com"));

    assert!(
        repo.read("CHANGELOG.md")
            .ends_with(&format!("[1.3.0]: {GH}/compare/v1.2.3...v1.3.0\n")),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn final_release_after_an_rc_moves_unreleased_from_the_last_final_tag() {
    let changelog = format!(
        "{TEMPLATE}\n## [1.2.3] - 2026-10-01\n\n- a\n\n[Unreleased]: {GH}/compare/v1.2.3...HEAD\n[1.2.3]: {GH}/releases/tag/v1.2.3\n"
    );
    let (repo, github) = github_repo(&changelog);
    assert_ok(&repo.kraken_on(&github, &["minor", "--pre", "rc", "--yes"], "github.com"));
    repo.commit("feat: add another command");

    assert_ok(&repo.kraken_on(&github, &["minor", "--yes"], "github.com"));

    assert!(
        repo.read("CHANGELOG.md").contains(&format!(
            "[Unreleased]: {GH}/compare/v1.3.0...HEAD\n[1.3.0]: {GH}/compare/v1.2.3...v1.3.0\n"
        )),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn pre_release_adds_no_link() {
    let changelog = format!(
        "{TEMPLATE}\n## [1.2.3] - 2026-10-01\n\n- a\n\n[Unreleased]: {GH}/compare/v1.2.3...HEAD\n[1.2.3]: {GH}/releases/tag/v1.2.3\n"
    );
    let (repo, github) = github_repo(&changelog);

    let dry = repo.kraken_on(
        &github,
        &["minor", "--pre", "rc", "--dry-run"],
        "github.com",
    );
    assert_ok(&dry);
    assert!(!stdout(&dry).contains("set the links"), "{}", stdout(&dry));

    assert_ok(&repo.kraken_on(&github, &["minor", "--pre", "rc", "--yes"], "github.com"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog);
}

#[test]
fn links_use_the_push_url_not_the_fetch_url() {
    let (repo, github) = github_repo(TEMPLATE);
    let mirror = bare_origin();
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        mirror.path().to_str().unwrap(),
    ]);
    repo.git(&[
        "remote",
        "set-url",
        "--push",
        "origin",
        "https://github.com/o/r.git",
    ]);

    let out = repo.kraken_on(&github, &["patch", "--dry-run"], "github.com");

    assert_ok(&out);
    assert!(
        stdout(&out).contains(&format!("[1.2.4]: {GH}/compare/v1.2.3...v1.2.4")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn trailing_blank_lines_stay_below_a_new_first_link() {
    let changelog = format!("{TEMPLATE}\n[docs]: https://example.com\n\n\n");
    let (repo, github) = github_repo(&changelog);

    github_release(&repo, &github);

    assert!(
        repo.read("CHANGELOG.md").ends_with(&format!(
            "[docs]: https://example.com\n[1.2.4]: {GH}/compare/v1.2.3...v1.2.4\n\n\n"
        )),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn no_changelog_flag_adds_no_links() {
    let (repo, github) = github_repo(TEMPLATE);

    assert_ok(&repo.kraken_on(&github, &["patch", "--yes", "--no-changelog"], "github.com"));

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
}

fn node_tagged(repo: &Repo) {
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
}

#[test]
fn summary_shows_the_changes_since_the_latest_tag() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    repo.commit("fix: b");

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains(
            "Current version: 1.2.3\nChanges: 1 feature, 1 fix since v1.2.3\nDry run, nothing changed."
        ),
        "{}",
        stdout(&out)
    );
}

#[test]
fn summary_counts_every_commit_on_the_first_release() {
    let repo = Repo::new();
    repo.write("VERSION", "0.1.0\n");
    repo.commit_all();

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Changes: 1 other commit since the first commit"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn summary_counts_the_commits_with_no_changelog() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: a");

    let out = repo.kraken(&["patch", "--dry-run", "--no-changelog"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Changes: 1 feature since v1.2.3"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn summary_counts_the_commits_without_a_changelog() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("fix: a");

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert!(
        stdout(&out).contains("Changes: 1 fix since v1.2.3"),
        "{}",
        stdout(&out)
    );
    assert!(!stdout(&out).contains("CHANGELOG.md"), "{}", stdout(&out));
}

#[test]
fn lower_level_than_the_commits_suggest_warns() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stderr(&out).contains("warning: the new commits suggest a minor release"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn lower_level_warns_and_still_releases_with_yes() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat!: a");

    let out = repo.kraken(&["minor", "--yes"]);

    assert_ok(&out);
    assert!(
        stderr(&out).contains("warning: the new commits suggest a major release"),
        "{}",
        stderr(&out)
    );
    assert_eq!(repo.tags(), ["v1.2.3", "v1.3.0"]);
}

#[test]
fn higher_level_than_the_commits_suggest_does_not_warn() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");

    for level in ["minor", "major"] {
        let out = repo.kraken(&[level, "--dry-run"]);

        assert_ok(&out);
        assert!(!stderr(&out).contains("suggest"), "{}", stderr(&out));
    }
}

#[test]
fn no_commits_since_the_tag_suggest_nothing() {
    let repo = Repo::new();
    node_tagged(&repo);

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Changes: none since v1.2.3"),
        "{}",
        stdout(&out)
    );
    assert!(!stderr(&out).contains("suggest"), "{}", stderr(&out));
}

#[test]
fn breaking_change_in_0x_suggests_minor() {
    let repo = Repo::new();
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("Cargo.lock", CARGO_LOCK);
    repo.write("src/lib.rs", "");
    repo.commit_all();
    repo.git(&["tag", "v0.1.0"]);
    repo.commit("feat!: a");

    let patch = repo.kraken(&["patch", "--dry-run"]);
    let minor = repo.kraken(&["minor", "--dry-run"]);

    assert!(
        stderr(&patch).contains("suggest a minor release"),
        "{}",
        stderr(&patch)
    );
    assert_ok(&minor);
    assert!(!stderr(&minor).contains("suggest"), "{}", stderr(&minor));
}

#[test]
fn merge_commits_do_not_count() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.git(&["switch", "-q", "-c", "topic"]);
    repo.commit("fix: on a branch");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "feat!: merge topic",
        "topic",
    ]);

    let out = repo.kraken(&["patch", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Changes: 1 fix since v1.2.3"),
        "{}",
        stdout(&out)
    );
    assert!(!stderr(&out).contains("suggest"), "{}", stderr(&out));
}

#[test]
fn auto_releases_minor_for_a_feature() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    repo.commit("fix: b");

    let out = repo.kraken(&["auto", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.3.0")
    );
    assert_eq!(repo.tag_commit("v1.3.0"), repo.head());
    assert!(stdout(&out).contains("Level: minor\n"), "{}", stdout(&out));
}

#[test]
fn auto_releases_major_for_a_breaking_change() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat(cli)!: rename a flag");

    assert_ok(&repo.kraken(&["auto", "--yes"]));

    assert_eq!(repo.tags(), ["v1.2.3", "v2.0.0"]);
}

#[test]
fn auto_reads_the_breaking_change_footer() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a\n\nBREAKING CHANGE: the flag is gone");

    assert_ok(&repo.kraken(&["auto", "--yes"]));

    assert_eq!(repo.tags(), ["v1.2.3", "v2.0.0"]);
}

#[test]
fn auto_releases_minor_for_a_breaking_change_in_0x() {
    let repo = Repo::new();
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("Cargo.lock", CARGO_LOCK);
    repo.write("src/lib.rs", "");
    repo.commit_all();
    repo.git(&["tag", "v0.1.0"]);
    repo.commit("feat!: a");

    let out = repo.kraken(&["auto", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.tags(), ["v0.1.0", "v0.2.0"]);
    assert!(
        stdout(&out).contains("Level: minor (breaking changes bump minor in 0.x)\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn auto_releases_patch_for_fixes_and_other_commits() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("fix: a");
    repo.commit("docs: b");
    repo.commit("Update README");

    let out = repo.kraken(&["auto", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.tags(), ["v1.2.3", "v1.2.4"]);
    assert!(stdout(&out).contains("Level: patch\n"), "{}", stdout(&out));
}

#[test]
fn auto_counts_every_commit_on_the_first_release() {
    let repo = Repo::new();
    repo.write("go.mod", "module demo\n");
    repo.commit_all();
    repo.commit("feat: serve over https");

    assert_ok(&repo.kraken(&["auto", "--yes"]));

    assert_eq!(repo.tags(), ["v0.1.0"]);
}

#[test]
fn auto_refuses_without_new_commits() {
    let repo = Repo::new();
    node_tagged(&repo);
    let head = repo.head();

    let out = repo.kraken(&["auto", "--yes"]);

    assert_failed_with(&out, "error: no commits since v1.2.3; nothing to release");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(repo.head(), head);
    assert_eq!(repo.tags(), ["v1.2.3"]);
    assert_eq!(repo.status(), "");
}

#[test]
fn auto_refuses_before_asking_origin() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.git(&["remote", "add", "origin", "/nonexistent/origin"]);

    let out = repo.kraken(&["auto", "--yes"]);

    assert_failed_with(&out, "nothing to release");
    assert!(
        !stdout(&out).contains("Checking tags on origin"),
        "{}",
        stdout(&out)
    );
    assert!(
        !stderr(&out).contains("could not compare tags with origin"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn auto_dry_run_shows_the_level_and_changes_nothing() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    let head = repo.head();

    let out = repo.kraken(&["auto", "--dry-run"]);

    assert_ok(&out);
    assert!(stdout(&out).contains("Level: minor\n"), "{}", stdout(&out));
    assert!(stdout(&out).contains("  tag v1.3.0\n"), "{}", stdout(&out));
    assert_eq!(repo.head(), head);
    assert_eq!(repo.tags(), ["v1.2.3"]);
    assert_eq!(repo.status(), "");
}

#[test]
fn auto_levels_line_follows_the_changes_line() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");

    let out = repo.kraken(&["auto", "--dry-run", "--no-changelog"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Changes: 1 feature since v1.2.3\nLevel: minor\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn auto_writes_the_same_changelog_section() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");

    assert_ok(&repo.kraken_dated(&["auto", "--yes"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!("{TEMPLATE}\n## [1.3.0] - 2026-11-04\n\n### Added\n\n- add a command\n")
    );
}

#[test]
fn missing_level_names_auto() {
    let repo = Repo::new();
    node_tagged(&repo);

    assert_failed_with(&repo.kraken(&[]), "pass patch, minor, major or auto");
}

#[test]
fn help_lists_auto() {
    let repo = Repo::new();

    let out = repo.kraken(&["--help"]);

    assert!(
        stdout(&out).contains("[possible values: patch, minor, major, auto]"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn release_without_new_commits_warns() {
    let repo = Repo::new();
    node_tagged(&repo);

    let dry = repo.kraken(&["patch", "--dry-run"]);
    assert_ok(&dry);
    assert!(
        stderr(&dry).contains("warning: no commits since v1.2.3\n"),
        "{}",
        stderr(&dry)
    );

    let out = repo.kraken(&["patch", "--yes"]);
    assert_ok(&out);
    assert!(
        stderr(&out).contains("warning: no commits since v1.2.3\n"),
        "{}",
        stderr(&out)
    );
    assert_eq!(repo.tags(), ["v1.2.3", "v1.2.4"]);
}

#[test]
fn release_warnings_list_the_empty_release_for_both_outputs() {
    let repo = Repo::new();
    node_tagged(&repo);

    let release = kraken::release::Release::prepare(repo.path(), true).unwrap();

    assert_eq!(release.warnings(None), ["no commits since v1.2.3"]);
}

#[test]
fn release_right_after_a_release_warns() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("fix: a");
    assert_ok(&repo.kraken(&["patch", "--yes"]));

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert!(
        stderr(&out).contains("warning: no commits since v1.2.4\n"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn auto_pre_release_refuses_to_release_the_same_commits_twice() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    assert_ok(&repo.kraken(&["auto", "--pre", "rc", "--yes"]));

    let out = repo.kraken(&["auto", "--pre", "rc", "--yes"]);

    assert_failed_with(
        &out,
        "error: no commits since v1.3.0-rc.1; nothing to release",
    );
    assert_eq!(repo.tags(), ["v1.2.3", "v1.3.0-rc.1"]);
}

#[test]
fn version_next_pre_refuses_without_commits_since_a_pre_release() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    assert_ok(&repo.kraken(&["auto", "--pre", "rc", "--yes"]));

    let out = repo.kraken(&["version", "--next", "--pre", "rc"]);

    assert_failed_with(
        &out,
        "error: no commits since v1.3.0-rc.1; nothing to release",
    );
}

#[test]
fn pre_release_right_after_a_pre_release_warns() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();
    repo.git(&["tag", "v1.2.3"]);
    repo.commit("feat: a");
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    let out = repo.kraken(&["--pre", "rc", "--yes"]);

    assert_ok(&out);
    assert!(
        stderr(&out).contains("warning: no commits since v1.3.0-rc.1\n"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn final_release_after_a_pre_release_counts_from_the_final_tag() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    let next = repo.kraken(&["version", "--next"]);
    let dry = repo.kraken(&["minor", "--dry-run"]);

    assert_ok(&next);
    assert_eq!(stdout(&next), "1.3.0\n");
    assert_ok(&dry);
    assert!(
        !stderr(&dry).contains("no commits since"),
        "{}",
        stderr(&dry)
    );
}

#[test]
fn release_with_new_commits_does_not_warn_about_commits() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("fix: a");

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert!(!stderr(&out).contains("no commits"), "{}", stderr(&out));
}

#[test]
fn first_release_does_not_warn() {
    let repo = Repo::new();
    repo.write("go.mod", "module demo\n");
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert!(!stderr(&out).contains("no commits"), "{}", stderr(&out));
}

fn composer_repo() -> Repo {
    let repo = Repo::new();
    repo.write("composer.json", COMPOSER_JSON);
    repo.write("composer.lock", COMPOSER_LOCK);
    repo
}

#[test]
fn composer_patch_release_updates_the_lock_hash() {
    let repo = composer_repo();
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("composer.json"),
        COMPOSER_JSON.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.read("composer.lock"),
        COMPOSER_LOCK.replace("ba6abe21b53dc5723447b7e88299a7b4", COMPOSER_HASH_124)
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "composer.json\ncomposer.lock"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
}

#[test]
fn composer_release_leaves_an_ignored_lock_out_of_the_commit() {
    let repo = composer_repo();
    repo.write(".gitignore", "composer.lock\n");
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("composer.lock"),
        COMPOSER_LOCK.replace("ba6abe21b53dc5723447b7e88299a7b4", COMPOSER_HASH_124)
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "composer.json"
    );
    assert_eq!(repo.status(), "");
}

#[test]
fn refuses_a_composer_lock_that_does_not_match() {
    let repo = composer_repo();
    repo.write(
        "composer.lock",
        &COMPOSER_LOCK.replace("ba6abe21b53dc5723447b7e88299a7b4", COMPOSER_HASH_124),
    );
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(
        &out,
        "composer.lock does not match composer.json; run `composer update --lock` first",
    );
    assert_eq!(repo.read("composer.json"), COMPOSER_JSON);
    assert_eq!(repo.status(), "");
    assert!(repo.tags().is_empty());
}

#[test]
fn composer_json_without_a_version_releases_from_tags() {
    let repo = Repo::new();
    repo.write("composer.json", "{ \"name\": \"acme/app\" }\n");
    repo.write("composer.lock", "{ \"content-hash\": \"stale\" }\n");
    repo.commit_all();
    repo.git(&["tag", "v0.4.0"]);
    let head = repo.head();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.tag_commit("v0.4.1"), head);
    assert_eq!(repo.head(), head);
}

const EXTENSION_JSON: &str =
    "{\n  \"manifest_version\": 3,\n  \"name\": \"Demo\",\n  \"version\": \"1.2.3\"\n}\n";

#[test]
fn web_extension_patch_release_with_package_json() {
    let repo = Repo::new();
    repo.write("manifest.json", EXTENSION_JSON);
    repo.node();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("manifest.json"),
        EXTENSION_JSON.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.read("package-lock.json"),
        PACKAGE_LOCK.replace("1.2.3", "1.2.4")
    );
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "manifest.json\npackage-lock.json\npackage.json"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
}

#[test]
fn web_app_manifest_leaves_a_node_release_alone() {
    const PWA: &str =
        "{\n  \"name\": \"App\",\n  \"start_url\": \"/\",\n  \"version\": \"9.9.9\"\n}\n";
    let repo = Repo::new();
    repo.write("manifest.json", PWA);
    repo.node();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.read("manifest.json"), PWA);
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "package-lock.json\npackage.json"
    );
}

#[test]
fn a_manifest_json_that_is_not_json_leaves_a_node_release_alone() {
    const JEKYLL: &str =
        "---\nlayout: null\n---\n{\n  \"name\": \"{{ site.title }}\",\n  \"start_url\": \"/\"\n}\n";
    let repo = Repo::new();
    repo.write("manifest.json", JEKYLL);
    repo.node();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.read("manifest.json"), JEKYLL);
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "package-lock.json\npackage.json"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
}

#[test]
fn refuses_a_two_number_extension_version() {
    let repo = Repo::new();
    repo.write("manifest.json", &EXTENSION_JSON.replace("1.2.3", "1.0"));
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(&out, "manifest.json: \"1.0\" is not an X.Y.Z version");
    assert_eq!(
        repo.read("manifest.json"),
        EXTENSION_JSON.replace("1.2.3", "1.0")
    );
    assert_eq!(repo.status(), "");
    assert!(repo.tags().is_empty());
}

const BUILD_PROPS: &str = "<Project>\n  <PropertyGroup>\n    <!-- <Version>0.9.0</Version> -->\n    <VersionPrefix>1.2.3</VersionPrefix>\n    <AssemblyVersion>1.0.0.0</AssemblyVersion>\n  </PropertyGroup>\n</Project>\n";
const VERSIONLESS_CSPROJ: &str = "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n";

#[test]
fn directory_build_props_patch_release() {
    let repo = Repo::new();
    repo.write("Directory.Build.props", BUILD_PROPS);
    repo.write("src/App/App.csproj", VERSIONLESS_CSPROJ);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("Directory.Build.props"),
        BUILD_PROPS.replace("<VersionPrefix>1.2.3<", "<VersionPrefix>1.2.4<")
    );
    assert_eq!(repo.read("src/App/App.csproj"), VERSIONLESS_CSPROJ);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.2.4"
    );
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn csproj_patch_release_keeps_the_bom() {
    let csproj = "\u{feff}<Project Sdk=\"Microsoft.NET.Sdk\">\r\n  <PropertyGroup>\r\n    <Version>1.2.3</Version>\r\n  </PropertyGroup>\r\n</Project>\r\n";
    let repo = Repo::new();
    repo.write("App.csproj", csproj);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.read("App.csproj"), csproj.replace("1.2.3", "1.2.4"));
    assert_eq!(repo.tag_commit("v1.2.4"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn refuses_disagreeing_directory_build_props_and_csproj() {
    let repo = Repo::new();
    repo.write("Directory.Build.props", BUILD_PROPS);
    repo.write(
        "App.csproj",
        "<Project><PropertyGroup><Version>1.3.0</Version></PropertyGroup></Project>",
    );
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(
        &out,
        "version files disagree: Directory.Build.props 1.2.3, App.csproj 1.3.0",
    );
}

#[test]
fn refuses_two_project_files_that_set_a_version() {
    let csproj = "<Project><PropertyGroup><Version>1.2.3</Version></PropertyGroup></Project>";
    let repo = Repo::new();
    repo.write("App.csproj", csproj);
    repo.write("Api.csproj", csproj);
    repo.commit_all();

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(
        &out,
        "more than one project file sets a version: Api.csproj, App.csproj",
    );
}

#[test]
fn minor_pre_release_from_a_final() {
    let repo = Repo::new();
    repo.node();

    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.3.0-rc.1")
    );
    assert_eq!(
        repo.read("package-lock.json"),
        PACKAGE_LOCK.replace("1.2.3", "1.3.0-rc.1")
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v1.3.0-rc.1"
    );
    assert_eq!(repo.tag_commit("v1.3.0-rc.1"), repo.head());
    assert_eq!(repo.status(), "");
}

#[test]
fn pre_release_counts_up_without_a_level() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_ok(&repo.kraken(&["--pre", "rc", "--yes"]));

    assert_eq!(repo.tags(), ["v1.3.0-rc.1", "v1.3.0-rc.2"]);
    assert!(repo.read("package.json").contains("\"1.3.0-rc.2\""));
}

#[test]
fn pre_release_moves_from_beta_to_rc() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "beta", "--yes"]));

    assert_ok(&repo.kraken(&["--pre", "rc", "--yes"]));

    assert_eq!(repo.tags(), ["v1.3.0-beta.1", "v1.3.0-rc.1"]);
}

#[test]
fn pre_release_cannot_go_back_from_rc_to_beta() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));
    let head = repo.head();

    let out = repo.kraken(&["--pre", "beta", "--yes"]);

    assert_failed_with(
        &out,
        "error: 1.3.0-beta.1 is lower than 1.3.0-rc.1; a pre-release goes from alpha to beta to rc",
    );
    assert_eq!(repo.head(), head);
    assert_eq!(repo.tags(), ["v1.3.0-rc.1"]);
    assert_eq!(repo.status(), "");
}

#[test]
fn pre_without_a_level_from_a_final_needs_a_level() {
    let repo = Repo::new();
    repo.node();

    let out = repo.kraken(&["--pre", "rc", "--yes"]);

    assert_failed_with(
        &out,
        "error: --pre without a level continues a pre-release, and 1.2.3 is not one; pass patch, minor, major or auto",
    );
    assert_eq!(repo.status(), "");
    assert!(repo.tags().is_empty());
}

#[test]
fn pre_takes_only_alpha_beta_and_rc() {
    let repo = Repo::new();
    repo.node();

    let out = repo.kraken(&["minor", "--pre", "preview", "--yes"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("[possible values: alpha, beta, rc]"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn final_release_after_pre_releases() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));
    assert_ok(&repo.kraken(&["--pre", "rc", "--yes"]));

    assert_ok(&repo.kraken(&["minor", "--yes"]));

    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.3.0")
    );
    assert_eq!(repo.tags(), ["v1.3.0", "v1.3.0-rc.1", "v1.3.0-rc.2"]);
}

#[test]
fn major_after_a_minor_pre_release_releases_the_next_major() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_ok(&repo.kraken(&["major", "--yes"]));

    assert!(repo.tags().contains(&"v2.0.0".to_string()));
}

#[test]
fn patch_from_a_minor_pre_release_is_refused() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    let out = repo.kraken(&["patch", "--yes"]);

    assert_failed_with(
        &out,
        "error: 1.3.0-rc.1 leads to 1.3.0, a minor release; pass minor or major",
    );
    assert_eq!(repo.tags(), ["v1.3.0-rc.1"]);
}

#[test]
fn pre_release_adds_no_changelog_section() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");

    assert_ok(&repo.kraken_dated(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert_eq!(
        repo.git(&["show", "--name-only", "--format=", "HEAD"]),
        "package-lock.json\npackage.json"
    );
}

#[test]
fn untracked_changelog_does_not_stop_a_pre_release() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    repo.write("CHANGELOG.md", TEMPLATE);

    assert_ok(&repo.kraken_dated(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
}

#[test]
fn final_release_lists_the_commits_since_the_last_final_release() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: first feature");
    assert_ok(&repo.kraken_dated(&["minor", "--pre", "rc", "--yes"]));
    repo.commit("fix: late fix");
    assert_ok(&repo.kraken_dated(&["--pre", "rc", "--yes"]));

    assert_ok(&repo.kraken_dated(&["minor", "--yes"]));

    let changelog = repo.read("CHANGELOG.md");
    assert!(changelog.contains("## [1.3.0] - 2026-11-04"), "{changelog}");
    assert!(changelog.contains("- first feature"), "{changelog}");
    assert!(changelog.contains("- late fix"), "{changelog}");
    assert!(!changelog.contains("rc.1"), "{changelog}");
    assert!(!changelog.contains("chore"), "{changelog}");
}

#[test]
fn dry_run_says_the_changelog_waits() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");

    let out = repo.kraken_dated(&["minor", "--pre", "rc", "--dry-run"]);

    assert_ok(&out);
    let out = stdout(&out);
    assert!(out.contains("  tag v1.3.0-rc.1\n"), "{out}");
    assert!(
        out.contains("  leave CHANGELOG.md alone: 1.3.0 gets the section\n"),
        "{out}"
    );
    assert!(!out.contains("add to CHANGELOG.md"), "{out}");
}

const PYPROJECT: &str =
    "[project]\nname = \"demo\"\nversion = \"1.3.0\"\nrequires-python = \">=3.9\"\n";
const UV_LOCK: &str = "version = 1\nrequires-python = \">=3.9\"\n\n[[package]]\nname = \"demo\"\nversion = \"1.3.0\"\nsource = { virtual = \".\" }\n";

fn python(repo: &Repo) {
    repo.write("pyproject.toml", PYPROJECT);
    repo.write("uv.lock", UV_LOCK);
    repo.commit_all();
}

#[test]
fn python_pre_release_uses_pep440() {
    let repo = Repo::new();
    python(&repo);

    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(
        repo.read("pyproject.toml"),
        PYPROJECT.replace("1.3.0", "1.4.0rc1")
    );
    assert_eq!(repo.read("uv.lock"), UV_LOCK.replace("1.3.0", "1.4.0rc1"));
    assert_eq!(repo.tags(), ["v1.4.0-rc.1"]);
    let out = repo.kraken(&["version"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "1.4.0-rc.1\n");
}

#[test]
fn python_pre_release_counts_up_from_the_pep440_spelling() {
    let repo = Repo::new();
    python(&repo);
    assert_ok(&repo.kraken(&["minor", "--pre", "beta", "--yes"]));
    assert!(repo.read("pyproject.toml").contains("1.4.0b1"));

    assert_ok(&repo.kraken(&["--pre", "rc", "--yes"]));
    assert_ok(&repo.kraken(&["--pre", "rc", "--yes"]));

    assert!(repo.read("pyproject.toml").contains("\"1.4.0rc2\""));
    assert_ok(&repo.kraken(&["minor", "--yes"]));
    assert!(repo.read("pyproject.toml").contains("\"1.4.0\""));
}

#[test]
fn python_dry_run_names_the_other_spelling() {
    let repo = Repo::new();
    python(&repo);

    let out = repo.kraken(&["minor", "--pre", "rc", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains(
            "  tag v1.4.0-rc.1\n  write 1.4.0rc1 to pyproject.toml, uv.lock: the PEP 440 spelling\n"
        ),
        "{}",
        stdout(&out)
    );
}

#[test]
fn python_in_another_pre_release_spelling_is_refused() {
    let repo = Repo::new();
    repo.write(
        "pyproject.toml",
        "[project]\nname = \"demo\"\nversion = \"1.4.0-rc.1\"\n",
    );
    repo.commit_all();

    assert_failed_with(
        &repo.kraken(&["patch", "--yes"]),
        "pyproject.toml: \"1.4.0-rc.1\" is not X.Y.Z or X.Y.Z with aN, bN or rcN, such as 1.4.0rc1",
    );
}

#[test]
fn version_files_in_two_spellings_agree() {
    let repo = Repo::new();
    repo.write("package.json", "{ \"version\": \"1.4.0-rc.1\" }\n");
    repo.write(
        "pyproject.toml",
        "[project]\nname = \"demo\"\nversion = \"1.4.0rc1\"\n",
    );
    repo.commit_all();

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.4.0-rc.1\n");
}

#[test]
fn version_prints_a_pre_release() {
    let repo = Repo::new();
    repo.write("VERSION", "1.4.0-rc.1\n");
    repo.commit_all();

    let out = repo.kraken(&["version"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.4.0-rc.1\n");
}

#[test]
fn rust_pre_release_updates_cargo_lock() {
    let repo = Repo::new();
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("Cargo.lock", CARGO_LOCK);
    repo.write("src/lib.rs", "");
    repo.commit_all();

    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(
        repo.read("Cargo.toml"),
        CARGO_TOML.replace("0.1.0", "0.2.0-rc.1")
    );
    assert_eq!(
        repo.read("Cargo.lock"),
        CARGO_LOCK.replace("0.1.0", "0.2.0-rc.1")
    );
    assert_eq!(repo.tags(), ["v0.2.0-rc.1"]);
}

#[test]
fn rust_workspace_pre_release_with_a_path_dependency_without_version() {
    let repo = Repo::new();
    rust_workspace(&repo);
    repo.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion.workspace = true\nedition = \"2021\"\n\n[dependencies]\nengine = { path = \"../engine\" }\n",
    );
    repo.git(&["commit", "-q", "-a", "-m", "drop the version"]);

    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(
        repo.read("Cargo.lock"),
        WORKSPACE_LOCK.replace("0.1.0", "0.2.0-rc.1")
    );
}

#[test]
fn xcode_refuses_a_pre_release() {
    const PBXPROJ: &str = "\t\tMARKETING_VERSION = 1.2.3;\n";
    let repo = Repo::new();
    repo.write("App.xcodeproj/project.pbxproj", PBXPROJ);
    repo.commit_all();

    let out = repo.kraken(&["minor", "--pre", "rc", "--yes"]);

    assert_failed_with(
        &out,
        "error: App.xcodeproj/project.pbxproj: MARKETING_VERSION takes numbers only, so it cannot be 1.3.0-rc.1",
    );
    assert_eq!(repo.read("App.xcodeproj/project.pbxproj"), PBXPROJ);
    assert_eq!(repo.status(), "");
    assert!(repo.tags().is_empty());
}

#[test]
fn xcodegen_refuses_a_pre_release() {
    let repo = Repo::new();
    repo.write("project.yml", PROJECT_YML);
    repo.commit_all();

    let out = repo.kraken(&["minor", "--pre", "rc", "--yes"]);

    assert_failed_with(
        &out,
        "error: project.yml: MARKETING_VERSION takes numbers only, so it cannot be 1.3.0-rc.1",
    );
    assert_eq!(repo.read("project.yml"), PROJECT_YML);
    assert!(repo.tags().is_empty());
}

#[test]
fn xcode_project_at_a_pre_release_is_refused_on_reading() {
    let repo = Repo::new();
    repo.write(
        "App.xcodeproj/project.pbxproj",
        "\t\tMARKETING_VERSION = 1.4.0-rc.1;\n",
    );
    repo.commit_all();

    assert_failed_with(
        &repo.kraken(&["version"]),
        "App.xcodeproj/project.pbxproj: \"1.4.0-rc.1\": MARKETING_VERSION takes numbers only",
    );
}

#[test]
fn composer_pre_release_with_an_unknown_label_is_refused() {
    let repo = Repo::new();
    repo.write(
        "composer.json",
        "{ \"name\": \"a/b\", \"version\": \"1.4.0-preview.1\" }\n",
    );
    repo.commit_all();

    assert_failed_with(
        &repo.kraken(&["version"]),
        "composer.json: \"1.4.0-preview.1\": Composer takes only the labels alpha, beta, RC, patch and dev",
    );
}

#[test]
fn web_extension_refuses_a_pre_release() {
    const MANIFEST: &str = "{ \"manifest_version\": 3, \"version\": \"1.2.3\" }\n";
    let repo = Repo::new();
    repo.write("manifest.json", MANIFEST);
    repo.commit_all();

    let out = repo.kraken(&["minor", "--pre", "rc", "--yes"]);

    assert_failed_with(
        &out,
        "error: manifest.json: a browser extension's version takes numbers only, so it cannot be 1.3.0-rc.1",
    );
    assert_eq!(repo.read("manifest.json"), MANIFEST);
}

#[test]
fn dotnet_version_prefix_refuses_a_pre_release() {
    const PROPS: &str =
        "<Project><PropertyGroup><VersionPrefix>1.2.3</VersionPrefix></PropertyGroup></Project>\n";
    let repo = Repo::new();
    repo.write("Directory.Build.props", PROPS);
    repo.commit_all();

    let out = repo.kraken(&["minor", "--pre", "rc", "--yes"]);

    assert_failed_with(
        &out,
        "error: Directory.Build.props: <VersionPrefix> holds X.Y.Z only, so it cannot be 1.3.0-rc.1",
    );
    assert_eq!(repo.read("Directory.Build.props"), PROPS);
}

#[test]
fn dotnet_version_prefix_refuses_a_pre_release_dry_run() {
    const PROPS: &str =
        "<Project><PropertyGroup><VersionPrefix>1.2.3</VersionPrefix></PropertyGroup></Project>\n";
    let repo = Repo::new();
    repo.write("Directory.Build.props", PROPS);
    repo.commit_all();

    let out = repo.kraken(&["minor", "--pre", "rc", "--dry-run"]);

    assert_failed_with(
        &out,
        "error: Directory.Build.props: <VersionPrefix> holds X.Y.Z only, so it cannot be 1.3.0-rc.1",
    );
}

#[test]
fn tag_only_pre_release_keeps_tag_style() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();
    repo.git(&["tag", "1.0.0"]);
    let head = repo.head();

    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));

    assert_eq!(repo.head(), head);
    assert_eq!(repo.tag_commit("1.1.0-rc.1"), head);
}

#[test]
fn tag_only_pre_release_continues_from_the_pre_release_tag() {
    let repo = Repo::new();
    repo.write("main.go", "package main\n");
    repo.commit_all();
    repo.git(&["tag", "v1.0.0"]);
    repo.commit("feat: a");
    repo.git(&["tag", "v1.1.0-rc.1"]);
    repo.commit("fix: b");

    assert_ok(&repo.kraken(&["--pre", "rc", "--yes"]));

    assert!(repo.tags().contains(&"v1.1.0-rc.2".to_string()));
}

#[test]
fn refuses_a_final_below_a_pre_release_tag() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.10.0"]);
    repo.git(&["tag", "v2.0.0-rc.1"]);

    assert_failed_with(
        &repo.kraken(&["patch", "--yes"]),
        "not higher than the latest tag v2.0.0-rc.1",
    );
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}

#[test]
fn refuses_when_origin_has_pre_release_tags_missing_here() {
    let source = Repo::new();
    source.write("main.go", "package main\n");
    source.commit_all();
    source.git(&["tag", "v1.4.0-rc.1"]);
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.path().display());
    git_in(clone.path(), &["clone", "-q", "--no-tags", &url, "repo"]);
    let repo = clone.path().join("repo");

    assert_failed_with(&kraken_in(&repo, &["patch", "--yes"]), "v1.4.0-rc.1");

    assert_eq!(git_in(&repo, &["tag", "--list"]), "");
}

#[test]
fn gradle_snapshot_releases_the_final_version() {
    let repo = Repo::new();
    repo.write("gradle.properties", "version=1.4.0-SNAPSHOT\n");
    repo.commit_all();

    assert_ok(&repo.kraken(&["minor", "--yes"]));

    assert_eq!(repo.read("gradle.properties"), "version=1.4.0\n");
    assert_eq!(repo.tags(), ["v1.4.0"]);
}

#[test]
fn summary_counts_the_commits_since_the_latest_final_tag() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    repo.commit("feat: a");
    assert_ok(&repo.kraken(&["minor", "--pre", "rc", "--yes"]));
    repo.commit("fix: b");

    let out = repo.kraken(&["--pre", "rc", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Changes: 1 feature, 1 fix, 1 other commit since v1.2.3"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn version_next_with_pre_prints_the_pre_release() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");

    let out = repo.kraken(&["version", "--next", "--pre", "rc"]);

    assert_ok(&out);
    assert_eq!(stdout(&out), "1.3.0-rc.1\n");
    let dry_run = stdout(&repo.kraken(&["auto", "--pre", "rc", "--dry-run"]));
    assert!(dry_run.contains("  tag v1.3.0-rc.1"), "{dry_run}");
}

#[test]
fn auto_pre_takes_the_level_from_the_commits() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("fix: a");

    assert_ok(&repo.kraken(&["auto", "--pre", "beta", "--yes"]));

    assert_eq!(repo.tags(), ["v1.2.3", "v1.2.4-beta.1"]);
}

#[test]
fn auto_from_a_pre_release_never_goes_below_its_level() {
    let repo = Repo::new();
    node_tagged(&repo);
    repo.commit("feat: a");
    assert_ok(&repo.kraken(&["auto", "--pre", "rc", "--yes"]));
    repo.commit("fix: b");

    assert_ok(&repo.kraken(&["auto", "--yes"]));

    assert!(repo.tags().contains(&"v1.3.0".to_string()));
}

#[test]
fn yes_and_dry_run_need_a_level_or_pre() {
    let repo = Repo::new();
    repo.node();

    for flag in ["--yes", "--dry-run"] {
        assert_failed_with(&repo.kraken(&[flag]), "<LEVEL|--pre <LABEL>>");
    }
}

#[test]
fn pre_alone_continues_a_pre_release_in_a_dry_run() {
    let repo = Repo::new();
    repo.node();
    assert_ok(&repo.kraken(&["minor", "--pre", "alpha", "--yes"]));

    let out = repo.kraken(&["--pre", "alpha", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("tag v1.3.0-alpha.2"),
        "{}",
        stdout(&out)
    );
}

fn node_released(repo: &Repo) {
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    assert_ok(&repo.kraken(&["patch", "--yes"]));
}

/// A Go project with a committed CHANGELOG.md and a feature commit.
fn go_with_changelog(repo: &Repo) {
    repo.write("CHANGELOG.md", TEMPLATE);
    go_module(repo);
    repo.commit("feat: add a command");
}

fn amend_with_a_new_file(repo: &Repo) {
    repo.write("src/lib.rs", "");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "--amend", "--no-edit"]);
    repo.git(&["tag", "-f", "-a", "v1.2.4", "-m", "v1.2.4"]);
}

#[test]
fn undo_removes_an_unpushed_release() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["tag", "v1.2.3"]);
    let before = repo.head();
    assert_ok(&repo.kraken(&["patch", "--yes"]));

    let out = repo.kraken(&["undo", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.head(), before);
    assert_eq!(repo.tags(), ["v1.2.3"]);
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
    assert_eq!(repo.read("package-lock.json"), PACKAGE_LOCK);
    assert_eq!(repo.status(), "");
    assert!(stdout(&out).contains("Dropped commit"), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("Deleted tag v1.2.4"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).trim_end().ends_with("Current version: 1.2.3"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn undo_puts_the_changelog_back() {
    let repo = Repo::new();
    repo.node_with_changelog();
    repo.commit("feat: add a command");
    let changelog = repo.read("CHANGELOG.md");
    assert_ok(&repo.kraken_dated(&["minor", "--yes"]));
    assert_ne!(repo.read("CHANGELOG.md"), changelog);

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.read("CHANGELOG.md"), changelog);
    assert_eq!(repo.status(), "");
}

#[test]
fn undo_removes_a_tag_only_release() {
    let repo = Repo::new();
    go_module(&repo);
    let head = repo.head();
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    assert_eq!(repo.tags(), ["v0.0.1"]);

    let out = repo.kraken(&["undo", "--yes"]);

    assert_ok(&out);
    assert!(repo.tags().is_empty());
    assert_eq!(repo.head(), head);
    assert!(!stdout(&out).contains("Dropped"), "{}", stdout(&out));
}

#[test]
fn undo_sets_an_untracked_lock_back() {
    let repo = Repo::new();
    repo.write(".gitignore", "Cargo.lock\n");
    repo.write("Cargo.toml", CARGO_TOML);
    repo.write("src/lib.rs", "");
    repo.commit_all();
    repo.write("Cargo.lock", CARGO_LOCK);
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    assert_eq!(
        repo.read("Cargo.lock"),
        CARGO_LOCK.replace("0.1.0", "0.1.1")
    );

    let out = repo.kraken(&["undo", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.read("Cargo.lock"), CARGO_LOCK);
    assert!(
        stdout(&out).contains("Set Cargo.lock to 0.1.0"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn undo_keeps_untracked_files() {
    let repo = Repo::new();
    repo.node();
    repo.write("notes.txt", "mine");
    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.read("notes.txt"), "mine");
    assert_eq!(repo.status(), "?? notes.txt");
}

#[test]
fn undo_removes_only_the_last_release() {
    let repo = Repo::new();
    node_released(&repo);
    let first = repo.head();
    assert_ok(&repo.kraken(&["patch", "--yes"]));

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.tags(), ["v1.2.3", "v1.2.4"]);
    assert_eq!(repo.head(), first);
    assert_eq!(
        repo.read("package.json"),
        PACKAGE_JSON.replace("1.2.3", "1.2.4")
    );

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.tags(), ["v1.2.3"]);
    assert_eq!(repo.read("package.json"), PACKAGE_JSON);
}

#[test]
fn undo_removes_one_of_two_tags_on_one_commit() {
    let repo = Repo::new();
    go_module(&repo);
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    assert_eq!(repo.tags(), ["v0.0.1", "v0.0.2"]);

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.tags(), ["v0.0.1"]);
}

#[test]
fn undo_removes_the_tag_only_release_above_a_release_commit() {
    let repo = Repo::new();
    go_with_changelog(&repo);
    let before = repo.head();
    assert_ok(&repo.kraken_dated(&["patch", "--yes"]));
    let release_commit = repo.head();
    assert_ok(&repo.kraken(&["patch", "--yes", "--no-changelog"]));
    assert_eq!(repo.tag_commit("v0.0.2"), release_commit);

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.tags(), ["v0.0.1"]);
    assert_eq!(repo.head(), release_commit);

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert!(repo.tags().is_empty());
    assert_eq!(repo.head(), before);
}

#[test]
fn undo_works_after_a_rejected_push() {
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
    let before = repo.head();
    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "stay local");

    let out = repo.kraken(&["undo", "--yes"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Checking origin..."),
        "{}",
        stdout(&out)
    );
    assert_eq!(repo.head(), before);
    assert!(repo.tags().is_empty());
}

#[test]
fn rejected_push_points_to_undo() {
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

    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "`kraken undo` removes");
}

#[test]
fn undo_works_when_origin_has_another_tag_with_the_same_name() {
    let repo = Repo::new();
    node_released(&repo);
    let origin = bare_origin();
    repo.add_origin(&origin);
    let other = Repo::new();
    other.commit("feat: elsewhere");
    other.add_origin(&origin);
    other.git(&["tag", "-a", "v1.2.4", "-m", "v1.2.4"]);
    other.git(&["push", "-q", "origin", "v1.2.4"]);

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert_eq!(repo.tags(), ["v1.2.3"]);
}

#[test]
fn undo_refuses_a_pushed_tag() {
    let repo = Repo::new();
    repo.node();
    let origin = bare_origin();
    repo.add_origin(&origin);
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    repo.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    let head = repo.head();

    let out = repo.kraken(&["undo", "--yes"]);

    assert_failed_with(&out, "tag v1.2.4 is already on origin");
    assert_eq!(repo.tags(), ["v1.2.4"]);
    assert_eq!(repo.head(), head);
}

#[test]
fn undo_refuses_a_release_commit_pushed_without_its_tag() {
    let repo = Repo::new();
    node_released(&repo);
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["push", "-q", "origin", "main"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "already on origin/main");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_asks_origin_for_its_branches() {
    let repo = Repo::new();
    node_released(&repo);
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["push", "-q", "origin", "main"]);
    repo.git(&["update-ref", "-d", "refs/remotes/origin/main"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "already on origin/main");
}

#[test]
fn undo_refuses_when_origin_is_unreachable() {
    let repo = Repo::new();
    node_released(&repo);
    repo.git(&["remote", "add", "origin", "/nonexistent/kraken-origin"]);

    let out = repo.kraken(&["undo", "--yes"]);

    assert_failed_with(&out, "could not check origin for v1.2.4");
    assert_failed_with(&out, "needs origin to answer");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_works_without_origin() {
    let repo = Repo::new();
    node_released(&repo);

    let out = repo.kraken(&["undo", "--yes"]);

    assert_ok(&out);
    assert!(
        !stdout(&out).contains("Checking origin"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn undo_refuses_commits_after_the_release() {
    let repo = Repo::new();
    node_released(&repo);
    repo.commit("fix: later");

    assert_failed_with(
        &repo.kraken(&["undo", "--yes"]),
        "the latest release v1.2.4 is not at HEAD; kraken undo removes only a release with no commits after it",
    );
}

#[test]
fn undo_refuses_an_amended_release_commit() {
    let repo = Repo::new();
    node_released(&repo);
    repo.git(&["commit", "-q", "--amend", "--allow-empty", "-m", "changed"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "not at HEAD");
}

#[test]
fn undo_refuses_a_release_commit_that_changes_other_files() {
    let repo = Repo::new();
    node_released(&repo);
    amend_with_a_new_file(&repo);

    let out = repo.kraken(&["undo", "--yes"]);

    assert_failed_with(&out, "changes files kraken does not write: src/lib.rs");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_refuses_a_lightweight_tag() {
    let repo = Repo::new();
    repo.node();
    repo.commit("chore(release): v1.2.4");
    repo.git(&["tag", "v1.2.4"]);

    assert_failed_with(
        &repo.kraken(&["undo", "--yes"]),
        "kraken did not make tag v1.2.4: kraken tags are annotated, with the tag name as the message",
    );
}

#[test]
fn undo_refuses_a_tag_with_another_message() {
    let repo = Repo::new();
    node_released(&repo);
    repo.git(&["tag", "-f", "-a", "v1.2.4", "-m", "Release 1.2.4"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "kraken did not make tag");
}

#[test]
fn undo_refuses_a_tag_on_a_commit_kraken_did_not_make() {
    let repo = Repo::new();
    repo.node();
    repo.commit("fix: a");
    repo.git(&["tag", "-a", "v1.2.4", "-m", "v1.2.4"]);

    assert_failed_with(
        &repo.kraken(&["undo", "--yes"]),
        "v1.2.4 is at HEAD, but HEAD is not its release commit \"chore(release): v1.2.4\"",
    );
    assert_eq!(repo.tags(), ["v1.2.4"]);
}

#[test]
fn undo_refuses_a_merge_with_the_release_subject() {
    let repo = Repo::new();
    repo.node();
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.commit("feat: side");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("feat: main");
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "chore(release): v1.2.4",
        "side",
    ]);
    repo.git(&["tag", "-a", "v1.2.4", "-m", "v1.2.4"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "is a merge");
}

#[test]
fn undo_refuses_uncommitted_changes() {
    let repo = Repo::new();
    node_released(&repo);
    repo.write("package.json", "{}");

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "uncommitted changes");
}

#[test]
fn undo_refuses_a_detached_head() {
    let repo = Repo::new();
    node_released(&repo);
    repo.git(&["checkout", "-q", "--detach"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "detached");
}

#[test]
fn undo_refuses_a_shallow_clone() {
    let source = Repo::new();
    node_released(&source);
    source.write("a.txt", "");
    source.commit_all();
    let (_holder, clone) = shallow_clone(&source);

    assert_failed_with(&kraken_in(&clone, &["undo", "--yes"]), "shallow");
}

#[test]
fn undo_has_nothing_to_undo_without_release_tags() {
    let repo = Repo::new();
    repo.node();

    assert_failed_with(
        &repo.kraken(&["undo", "--yes"]),
        "there is no release tag to undo",
    );
}

#[test]
fn undo_dry_run_changes_nothing() {
    let repo = Repo::new();
    node_released(&repo);
    let head = repo.head();

    let out = repo.kraken(&["undo", "--dry-run"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains("Dry run, nothing changed. kraken undo would:"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("  delete tag v1.2.4"),
        "{}",
        stdout(&out)
    );
    assert_eq!(repo.head(), head);
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_dry_run_wins_over_yes() {
    let repo = Repo::new();
    node_released(&repo);

    assert_ok(&repo.kraken(&["undo", "--yes", "--dry-run"]));

    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_dry_run_asks_origin() {
    let repo = Repo::new();
    node_released(&repo);
    repo.git(&["remote", "add", "origin", "/nonexistent/kraken-origin"]);

    let out = repo.kraken(&["undo", "--dry-run"]);

    assert_failed_with(&out, "needs origin to answer");
}

#[test]
fn undo_dry_run_still_refuses() {
    let repo = Repo::new();
    node_released(&repo);
    repo.commit("fix: later");

    assert_failed_with(&repo.kraken(&["undo", "--dry-run"]), "not at HEAD");
}

#[test]
fn undo_asks_only_in_a_terminal() {
    let repo = Repo::new();
    node_released(&repo);

    let out = repo.kraken(&["undo"]);

    assert_failed_with(
        &out,
        "confirming needs stdin and stdout to be a terminal; pass --yes",
    );
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_takes_no_level() {
    let repo = Repo::new();
    node_released(&repo);

    let out = repo.kraken(&["undo", "patch"]);

    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn undo_refuses_when_the_repository_changed_after_the_start() {
    let repo = Repo::new();
    node_released(&repo);
    let undo = kraken::undo::Undo::prepare(repo.path()).unwrap();
    repo.write("notes.md", "");
    repo.commit_all();
    let head = repo.head();

    let err = undo.apply().unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert_eq!(repo.head(), head);
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_refuses_uncommitted_changes_made_after_the_start() {
    let repo = Repo::new();
    repo.node();
    repo.write("notes.txt", "a");
    repo.commit_all();
    repo.git(&["tag", "v1.2.3"]);
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    let undo = kraken::undo::Undo::prepare(repo.path()).unwrap();
    repo.write("notes.txt", "b");
    let head = repo.head();

    let err = undo.apply().unwrap_err().to_string();

    assert!(err.contains("uncommitted changes"), "{err}");
    assert_eq!(repo.head(), head);
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_refuses_when_the_tag_moved_after_the_start() {
    let repo = Repo::new();
    node_released(&repo);
    let undo = kraken::undo::Undo::prepare(repo.path()).unwrap();
    repo.git(&["tag", "-f", "-a", "v1.2.4", "-m", "v1.2.4", "HEAD~1"]);
    let head = repo.head();

    let err = undo.apply().unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert_eq!(repo.head(), head);
}

#[test]
fn undo_refuses_when_the_branch_changed_after_the_start() {
    let repo = Repo::new();
    node_released(&repo);
    let undo = kraken::undo::Undo::prepare(repo.path()).unwrap();
    repo.git(&["checkout", "-q", "-b", "other"]);
    let head = repo.head();

    let err = undo.apply().unwrap_err().to_string();

    assert!(err.contains("changed since kraken started"), "{err}");
    assert_eq!(repo.head(), head);
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_refuses_a_release_commit_that_is_the_first_commit() {
    let repo = Repo::new();
    repo.write("package.json", PACKAGE_JSON);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "chore(release): v1.2.4"]);
    repo.git(&["tag", "-a", "v1.2.4", "-m", "v1.2.4"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "is the first commit");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_works_when_origin_moved_on_to_a_commit_this_clone_lacks() {
    let repo = Repo::new();
    repo.node();
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["push", "-q", "origin", "main"]);
    let holder = tempfile::tempdir().unwrap();
    let url = format!("file://{}", origin.path().display());
    git_in(holder.path(), &["clone", "-q", "-b", "main", &url, "other"]);
    let other = holder.path().join("other");
    git_in(
        &other,
        &["commit", "-q", "--allow-empty", "-m", "feat: elsewhere"],
    );
    git_in(&other, &["push", "-q", "origin", "main"]);
    let before = repo.head();
    assert_failed_with(&repo.kraken(&["patch", "--yes"]), "stay local");

    let out = repo.kraken(&["undo", "--yes"]);

    assert_ok(&out);
    assert_eq!(repo.head(), before);
    assert!(repo.tags().is_empty());
}

#[test]
fn undo_refuses_a_release_commit_pushed_to_another_remote() {
    let repo = Repo::new();
    node_released(&repo);
    let upstream = bare_origin();
    repo.git(&[
        "remote",
        "add",
        "upstream",
        upstream.path().to_str().unwrap(),
    ]);
    repo.git(&["push", "-q", "upstream", "main"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "already on upstream/main");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_refuses_when_an_origin_branch_is_ahead_of_the_release_commit() {
    let repo = Repo::new();
    node_released(&repo);
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.commit("feat: after the release");
    repo.git(&["push", "-q", "origin", "side:main"]);
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["update-ref", "-d", "refs/remotes/origin/main"]);

    assert_failed_with(&repo.kraken(&["undo", "--yes"]), "already on origin/main");
    assert!(repo.tags().contains(&"v1.2.4".to_string()));
}

#[test]
fn undo_removes_a_tag_only_release_whose_commit_is_on_origin() {
    let repo = Repo::new();
    go_module(&repo);
    assert_ok(&repo.kraken(&["patch", "--yes"]));
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["push", "-q", "origin", "main"]);

    assert_ok(&repo.kraken(&["undo", "--yes"]));

    assert!(repo.tags().is_empty());
}

#[test]
fn undo_names_the_branch_not_origin_head() {
    let repo = Repo::new();
    node_released(&repo);
    let origin = bare_origin();
    repo.add_origin(&origin);
    repo.git(&["push", "-q", "origin", "main"]);
    repo.git(&["remote", "set-head", "origin", "main"]);

    assert_failed_with(
        &repo.kraken(&["undo", "--yes"]),
        "already on origin/main; kraken",
    );
}

const THREE_SECTIONS: &str = "## [1.1.0] - 2026-10-01\n\
     \n\
     ### Added\n\
     \n\
     - **cli**: add a command\n\
     \n\
     ## [1.0.1] - 2026-09-20\n\
     \n\
     ## [1.0.0] - 2026-09-01\n\
     \n\
     ### Changed\n\
     \n\
     - init\n";

#[test]
fn changelog_fills_in_a_section_for_every_release() {
    let repo = Repo::new();
    repo.three_releases();

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!("{TEMPLATE}\n{THREE_SECTIONS}")
    );
}

#[test]
fn changelog_says_how_many_releases_it_filled_in() {
    let repo = Repo::new();
    repo.commit_at("init", "2026-09-01T10:00:00+0000");
    repo.tag_at("v1.0.0", "2026-09-01T10:00:00+0000");
    let out = repo.kraken(&["changelog"]);
    assert_ok(&out);
    assert_eq!(
        stdout(&out),
        "Created CHANGELOG.md with 1 release. Commit it, and every release adds a section to it.\n"
    );

    let repo = Repo::new();
    repo.three_releases();
    let out = repo.kraken(&["changelog"]);
    assert_ok(&out);
    assert_eq!(
        stdout(&out),
        "Created CHANGELOG.md with 3 releases. Commit it, and every release adds a section to it.\n"
    );
}

#[test]
fn changelog_gives_the_lowest_tag_every_commit_before_it() {
    let repo = Repo::new();
    repo.commit_at("fix: first", "2026-08-01T10:00:00+0000");
    repo.commit_at("feat: second", "2026-08-02T10:00:00+0000");
    repo.commit_at("third", "2026-08-03T10:00:00+0000");
    repo.tag_at("v0.1.0", "2026-08-04T10:00:00+0000");

    assert_ok(&repo.kraken(&["changelog"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{TEMPLATE}\n## [0.1.0] - 2026-08-04\n\n### Added\n\n- second\n\n### Changed\n\n- third\n\n### Fixed\n\n- first\n"
        )
    );
}

#[test]
fn changelog_dates_a_section_by_its_tag() {
    let repo = Repo::new();
    repo.commit_at("one", "2026-10-01T10:00:00+0000");
    repo.tag_at("v1.0.0", "2026-10-03T10:00:00+0000");
    repo.commit_at("two", "2026-11-04T23:30:00-0500");
    repo.light_tag("v1.1.0");

    let out = isolate(Command::new(env!("CARGO_BIN_EXE_kraken")).arg("changelog"))
        .env("TZ", "Asia/Tokyo")
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert_ok(&out);
    let changelog = repo.read("CHANGELOG.md");
    assert!(
        changelog.contains("## [1.0.0] - 2026-10-03\n"),
        "{changelog}"
    );
    assert!(
        changelog.contains("## [1.1.0] - 2026-11-04\n"),
        "{changelog}"
    );
}

#[test]
fn changelog_history_leaves_out_release_commits_and_merges() {
    let repo = Repo::new();
    repo.commit_at("feat: base", "2026-09-01T10:00:00+0000");
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.commit_at("fix: on the side", "2026-09-02T10:00:00+0000");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit_at("feat: on main", "2026-09-03T10:00:00+0000");
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge the side", "side"]);
    repo.commit_at("chore(release): v1.1.0", "2026-09-04T10:00:00+0000");
    repo.tag_at("v1.1.0", "2026-09-04T10:00:00+0000");

    assert_ok(&repo.kraken(&["changelog"]));

    let changelog = repo.read("CHANGELOG.md");
    assert!(changelog.contains("- on the side"), "{changelog}");
    assert!(!changelog.contains("Merge the side"), "{changelog}");
    assert!(!changelog.contains("chore(release)"), "{changelog}");
    assert!(!changelog.contains("v1.1.0\n"), "{changelog}");
}

#[test]
fn changelog_history_skips_tags_that_are_not_releases() {
    let repo = Repo::new();
    repo.commit_at("feat: base", "2026-09-01T10:00:00+0000");
    repo.tag_at("v1.0.0", "2026-09-01T10:00:00+0000");
    repo.git(&["checkout", "-q", "-b", "elsewhere"]);
    repo.commit_at("feat: never merged", "2026-09-02T10:00:00+0000");
    repo.tag_at("v1.0.5", "2026-09-02T10:00:00+0000");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit_at("fix: before the candidate", "2026-09-03T10:00:00+0000");
    repo.tag_at("v1.1.0-rc.1", "2026-09-03T10:00:00+0000");
    repo.commit_at("fix: after the candidate", "2026-09-04T10:00:00+0000");
    repo.light_tag("nightly");
    repo.tag_at("v1.1.0", "2026-09-05T10:00:00+0000");

    assert_ok(&repo.kraken(&["changelog"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{TEMPLATE}\n## [1.1.0] - 2026-09-05\n\n### Fixed\n\n- before the candidate\n- after the candidate\n\n## [1.0.0] - 2026-09-01\n\n### Added\n\n- base\n"
        )
    );
}

#[test]
fn changelog_history_counts_tags_on_merged_branches() {
    let repo = Repo::new();
    repo.commit_at("feat: base", "2026-09-01T10:00:00+0000");
    repo.tag_at("v1.0.0", "2026-09-01T10:00:00+0000");
    repo.git(&["checkout", "-q", "-b", "maintenance"]);
    repo.commit_at("fix: backport", "2026-09-02T10:00:00+0000");
    repo.tag_at("v1.0.1", "2026-09-02T10:00:00+0000");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit_at("feat: one", "2026-09-03T10:00:00+0000");
    repo.tag_at("v1.1.0", "2026-09-03T10:00:00+0000");
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge maintenance",
        "maintenance",
    ]);
    repo.commit_at("feat: two", "2026-09-05T10:00:00+0000");
    repo.tag_at("v1.2.0", "2026-09-06T10:00:00+0000");

    assert_ok(&repo.kraken(&["changelog"]));

    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{TEMPLATE}\n\
             ## [1.2.0] - 2026-09-06\n\n### Added\n\n- two\n\n### Fixed\n\n- backport\n\n\
             ## [1.1.0] - 2026-09-03\n\n### Added\n\n- one\n\n\
             ## [1.0.1] - 2026-09-02\n\n### Fixed\n\n- backport\n\n\
             ## [1.0.0] - 2026-09-01\n\n### Added\n\n- base\n"
        )
    );
}

#[test]
fn changelog_history_leaves_out_commits_after_the_latest_tag() {
    let repo = Repo::new();
    repo.commit_at("feat: released", "2026-09-01T10:00:00+0000");
    repo.tag_at("v1.0.0", "2026-09-01T10:00:00+0000");
    repo.commit_at("feat: not released", "2026-09-02T10:00:00+0000");

    assert_ok(&repo.kraken(&["changelog"]));

    let changelog = repo.read("CHANGELOG.md");
    assert!(changelog.contains("- released"), "{changelog}");
    assert!(!changelog.contains("not released"), "{changelog}");
}

#[test]
fn changelog_without_commits_writes_the_template() {
    let repo = Repo::new();

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert_eq!(
        stdout(&out),
        "Created CHANGELOG.md. Commit it, and every release adds a section to it.\n"
    );
}

#[test]
fn changelog_without_release_tags_keeps_the_message() {
    let repo = Repo::new();
    repo.commit_at("init", "2026-09-01T10:00:00+0000");
    repo.light_tag("nightly");

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert_eq!(repo.read("CHANGELOG.md"), TEMPLATE);
    assert_eq!(
        stdout(&out),
        "Created CHANGELOG.md. Commit it, and every release adds a section to it.\n"
    );
}

#[test]
fn changelog_refuses_a_shallow_clone() {
    let source = Repo::new();
    source.three_releases();
    let (_holder, path) = shallow_clone(&source);

    let out = kraken_in(&path, &["changelog"]);

    assert_failed_with(
        &out,
        "error: this is a shallow clone and may miss tags; run `git fetch --unshallow --tags` first",
    );
    assert!(!path.join("CHANGELOG.md").exists());
}

#[test]
fn changelog_history_dates_a_tag_without_a_tagger_by_its_commit() {
    let repo = Repo::new();
    repo.commit_at("init", "2026-09-01T10:00:00+0000");
    let commit = repo.git(&["rev-parse", "HEAD"]);
    repo.write(
        ".git/tagobject",
        &format!("object {commit}\ntype commit\ntag v1.0.0\n\nv1.0.0\n"),
    );
    let tag = repo.git(&[
        "hash-object",
        "-t",
        "tag",
        "-w",
        "--literally",
        ".git/tagobject",
    ]);
    repo.git(&["update-ref", "refs/tags/v1.0.0", &tag]);

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert!(
        repo.read("CHANGELOG.md")
            .contains("## [1.0.0] - 2026-09-01\n"),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn changelog_in_a_shallow_clone_checks_the_existing_file() {
    let source = Repo::new();
    source.write("CHANGELOG.md", TEMPLATE);
    source.git(&["add", "-A"]);
    source.commit_at("init", "2026-09-01T10:00:00+0000");
    let (_holder, path) = shallow_clone(&source);

    let out = kraken_in(&path, &["changelog"]);

    assert_ok(&out);
    assert!(
        stdout(&out).contains(
            "CHANGELOG.md has the kraken line. Once it is committed, every release adds a section to it."
        ),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        fs::read_to_string(path.join("CHANGELOG.md")).unwrap(),
        TEMPLATE
    );
}

#[test]
fn changelog_history_does_not_ask_origin() {
    let repo = Repo::new();
    repo.three_releases();
    repo.set_origin("/no/such/place.git");

    let out = repo.kraken(&["changelog"]);

    assert_ok(&out);
    assert_eq!(stderr(&out), "");
    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!("{TEMPLATE}\n{THREE_SECTIONS}")
    );
}

#[test]
fn changelog_history_links_every_release() {
    let repo = Repo::new();
    repo.commit_at("init", "2026-09-01T10:00:00+0000");
    repo.tag_at("1.0.0", "2026-09-01T10:00:00+0000");
    repo.commit_at("fix: a", "2026-09-02T10:00:00+0000");
    repo.tag_at("v1.1.0", "2026-09-02T10:00:00+0000");
    let github = FakeGitHub::new();
    repo.add_github_origin(&github);

    let out = repo.kraken_on(&github, &["changelog"], "github.com");

    assert_ok(&out);
    assert!(
        repo.read("CHANGELOG.md").ends_with(&format!(
            "\n[1.1.0]: {GH}/compare/1.0.0...v1.1.0\n[1.0.0]: {GH}/releases/tag/1.0.0\n"
        )),
        "{}",
        repo.read("CHANGELOG.md")
    );
}

#[test]
fn release_after_changelog_continues_the_history() {
    let repo = Repo::new();
    repo.commit_at("init", "2026-09-01T10:00:00+0000");
    repo.tag_at("v1.0.0", "2026-09-01T10:00:00+0000");
    repo.commit_at("fix: a", "2026-09-02T10:00:00+0000");
    repo.tag_at("v1.0.1", "2026-09-02T10:00:00+0000");
    let github = FakeGitHub::new();
    repo.add_github_origin(&github);
    assert_ok(&repo.kraken_on(&github, &["changelog"], "github.com"));
    repo.git(&["add", "CHANGELOG.md"]);
    repo.commit_at("docs: add the changelog", "2026-10-01T10:00:00+0000");
    repo.commit_at("feat: new thing", "2026-10-02T10:00:00+0000");

    assert_ok(&repo.kraken_on(&github, &["minor", "--yes"], "github.com"));

    let changelog = repo.read("CHANGELOG.md");
    let at = |text: &str| {
        changelog
            .find(text)
            .unwrap_or_else(|| panic!("{text}\n{changelog}"))
    };
    assert!(at("## [1.1.0] - 2026-11-04") < at("## [1.0.1] - 2026-09-02"));
    assert!(
        at(&format!("[1.1.0]: {GH}/compare/v1.0.1...v1.1.0"))
            < at(&format!("[1.0.1]: {GH}/compare/v1.0.0...v1.0.1"))
    );
}
