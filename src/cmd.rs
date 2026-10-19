use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

/// Runs a program in `dir` and returns its trimmed stdout.
pub fn run(program: &str, args: &[&str], dir: &Path) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(dir);
    let out = command
        .output()
        .with_context(|| format!("could not run {program}"))?;
    stdout_of(&describe(&command), out)
}

/// Like [`run`], but stops the program, and everything it started, when it runs longer than `limit`.
pub fn run_with_limit(command: &mut Command, limit: Duration) -> Result<String> {
    let name = describe(command);
    // Its own process group keeps it away from the terminal and lets us kill all it started.
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .with_context(|| format!("could not run {name}"))?;
    // Read the pipes while waiting, or a program with a lot of output blocks on a full pipe.
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let deadline = Instant::now() + limit;
    let timed_out = || anyhow!("`{name}` did not finish in {} s", limit.as_secs_f32());

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_group(&child);
            child.wait()?;
            return Err(timed_out());
        }
        thread::sleep(Duration::from_millis(20));
    };
    // A program it started in the background may still hold the pipes open.
    let left = || deadline.saturating_duration_since(Instant::now());
    let (Ok(stdout), Ok(stderr)) = (stdout.recv_timeout(left()), stderr.recv_timeout(left()))
    else {
        kill_group(&child);
        return Err(timed_out());
    };
    stdout_of(
        &name,
        Output {
            status,
            stdout,
            stderr,
        },
    )
}

fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error leaves what was read so far, which is all there is to show.
            let _ = pipe.read_to_end(&mut bytes);
        }
        // The receiver is gone when the caller gave up waiting.
        let _ = sender.send(bytes);
    });
    receiver
}

/// Kills the process group that `child` leads. The group may be gone already.
fn kill_group(child: &Child) {
    let group = format!("-{}", child.id());
    let _ = Command::new("kill").args(["-KILL", "--", &group]).output();
}

fn describe(command: &Command) -> String {
    let program = command.get_program().to_string_lossy();
    let args = command.get_args().map(|arg| arg.to_string_lossy());
    std::iter::once(program)
        .chain(args)
        .collect::<Vec<_>>()
        .join(" ")
}

fn stdout_of(command: &str, out: Output) -> Result<String> {
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let reason = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|text| !text.is_empty());
        match reason {
            Some(reason) => bail!("`{command}` failed: {reason}"),
            None => bail!("`{command}` failed"),
        }
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    fn alive(pid: &str) -> bool {
        Command::new("kill")
            .args(["-0", pid])
            .output()
            .unwrap()
            .status
            .success()
    }

    #[test]
    fn stops_a_program_that_runs_too_long() {
        let started = Instant::now();

        let err = run_with_limit(&mut sh("sleep 5"), Duration::from_millis(200))
            .unwrap_err()
            .to_string();

        assert!(err.contains("did not finish in"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stopping_also_stops_what_the_program_started() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 30 & echo $! > {}; wait", pid_file.display());

        run_with_limit(&mut sh(&script), Duration::from_millis(300)).unwrap_err();

        let pid = fs::read_to_string(&pid_file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while alive(pid.trim()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(pid.trim()), "background sleep {pid} survived");
    }

    #[test]
    fn does_not_wait_past_the_limit_for_background_children() {
        let started = Instant::now();

        let result = run_with_limit(&mut sh("sleep 5 & echo hi"), Duration::from_millis(500));

        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn limited_run_returns_large_stdout() {
        let out =
            run_with_limit(&mut sh("yes x | head -n 100000"), Duration::from_secs(5)).unwrap();
        assert_eq!(out.lines().count(), 100_000);
    }

    #[test]
    fn limited_run_reports_failure() {
        let err = run_with_limit(&mut sh("echo bad >&2; exit 1"), Duration::from_secs(5))
            .unwrap_err()
            .to_string();
        assert!(err.ends_with("failed: bad"), "{err}");
    }

    #[test]
    fn failure_shows_stdout_when_stderr_is_empty() {
        let err = run(
            "sh",
            &["-c", "echo nothing to commit; exit 1"],
            Path::new("."),
        )
        .unwrap_err()
        .to_string();
        assert!(err.ends_with("failed: nothing to commit"), "{err}");
    }

    #[test]
    fn failure_without_output_ends_cleanly() {
        let err = run("sh", &["-c", "exit 1"], Path::new("."))
            .unwrap_err()
            .to_string();
        assert!(err.ends_with("failed"), "{err}");
    }
}
