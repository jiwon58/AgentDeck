use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::{env, ffi::OsString, fs, io, path::PathBuf, process::Command};

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Agent Deck Team engine: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> io::Result<i32> {
    let registry = env::var_os("PSMUX_DATA_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.join("psmux.conf").is_file())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing Deck Team session"))?;
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let bridge = env::current_exe()?;
    if let [mode, payload] = args.as_slice() {
        if mode == "--worker" {
            let prompt = URL_SAFE_NO_PAD
                .decode(payload.to_string_lossy().as_bytes())
                .map_err(io::Error::other)?;
            let prompt = String::from_utf8(prompt).map_err(io::Error::other)?;
            let executable = registered_worker()?;
            let digest = env::var("DECK_TEAM_GJC_SHA256").map_err(io::Error::other)?;
            let candidate = agent_deck::registry::inspect(&executable).map_err(io::Error::other)?;
            if candidate.sha256 != digest {
                return Err(io::Error::other(
                    "The registered GJC worker executable changed",
                ));
            }
            return Command::new(candidate.path)
                .arg(prompt)
                .status()
                .map(|status| status.code().unwrap_or(1));
        }
    }
    let mut engine = Command::new(bridge.with_file_name("psmux.exe"));
    if let [command, literal, target_flag, target, input] = args.as_slice() {
        if command == "send-keys" && literal == "-l" && target_flag == "-t" {
            if let Some((pane, input)) = pane_id(target).zip(input.to_str()) {
                if input.starts_with("& { $env:GJC_TEAM_WORKER = ") {
                    let gjc = registered_worker()?;
                    let worker = Worker::parse(input, &gjc).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Unexpected GJC Team worker command",
                        )
                    })?;
                    let mut respawn = engine.args(["respawn-pane", "-k", "-t"]).arg(target);
                    respawn = respawn.arg("-c").arg(worker.cwd);
                    for (key, value) in worker.environment {
                        respawn = respawn.arg("-e").arg(format!("{key}={value}"));
                    }
                    // Keep the vendor prompt out of psmux's shell detection.
                    let payload = URL_SAFE_NO_PAD.encode(worker.prompt.as_bytes());
                    let status = respawn
                        .arg("--")
                        .arg(&bridge)
                        .args(["--worker", &payload])
                        .status()?;
                    if status.success() {
                        // GJC sends Enter in a second call. The worker has already been
                        // started with argv, so consume that one submission.
                        fs::write(registry.join(format!("worker-enter-{pane}")), [])?;
                    }
                    return Ok(status.code().unwrap_or(1));
                }
            }
        }
    }
    if let [command, target_flag, target, key] = args.as_slice() {
        if command == "send-keys" && target_flag == "-t" && key == "Enter" {
            if let Some(pane) = pane_id(target) {
                if fs::remove_file(registry.join(format!("worker-enter-{pane}"))).is_ok() {
                    return Ok(0);
                }
            }
        }
    }
    engine
        .args(args)
        .status()
        .map(|status| status.code().unwrap_or(1))
}

fn registered_worker() -> io::Result<String> {
    env::var("DECK_TEAM_GJC")
        .ok()
        .filter(|path| PathBuf::from(path).is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing registered GJC worker runtime",
            )
        })
}

fn pane_id(target: &OsString) -> Option<u32> {
    target.to_str()?.strip_prefix('%')?.parse().ok()
}

struct Worker<'a> {
    cwd: String,
    environment: Vec<(&'a str, String)>,
    prompt: String,
}

impl<'a> Worker<'a> {
    fn parse(input: &'a str, gjc: &str) -> Option<Self> {
        let mut remaining = input.strip_prefix("& { ")?;
        let mut environment = Vec::new();
        while let Some(assignment) = remaining.strip_prefix("$env:") {
            let (key, value) = assignment.split_once(" = ")?;
            if !matches!(
                key,
                "GJC_TEAM_WORKER"
                    | "GJC_TEAM_INTERNAL_WORKER"
                    | "GJC_TEAM_NAME"
                    | "GJC_TEAM_WORKER_ID"
                    | "GJC_TEAM_STATE_ROOT"
                    | "GJC_TEAM_LEADER_CWD"
                    | "GJC_TEAM_DISPLAY_NAME"
                    | "GJC_SPAWNED_BY_SESSION"
                    | "GJC_TEAM_WORKTREE_PATH"
            ) || environment.iter().any(|(seen, _)| *seen == key)
            {
                return None;
            }
            let (value, tail) = single_quoted(value)?;
            environment.push((key, value));
            remaining = tail.strip_prefix("; ")?;
        }
        let (executable, tail) = single_quoted(remaining.strip_prefix("& ")?)?;
        if executable != gjc || !PathBuf::from(&executable).is_absolute() {
            return None;
        }
        let (prompt, tail) = single_quoted(tail.strip_prefix(' ')?)?;
        if tail != " }" {
            return None;
        }
        let cwd = ["GJC_TEAM_WORKTREE_PATH", "GJC_TEAM_LEADER_CWD"]
            .into_iter()
            .find_map(|name| environment.iter().find(|(key, _)| *key == name))
            .map(|(_, value)| value.to_owned())?;
        if !PathBuf::from(&cwd).is_absolute() {
            return None;
        }
        Some(Self {
            cwd,
            environment,
            prompt,
        })
    }
}

fn single_quoted(input: &str) -> Option<(String, &str)> {
    let mut input = input.strip_prefix('\'')?;
    let mut value = String::new();
    loop {
        let (text, tail) = input.split_once('\'')?;
        value.push_str(text);
        let Some(after_escape) = tail.strip_prefix('\'') else {
            return Some((value, tail));
        };
        value.push('\'');
        input = after_escape;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn accepts_only_the_registered_worker_executable_and_literal_prompt() {
        let path = std::env::current_dir().unwrap().join("GJC's copy.exe");
        let gjc = path.to_str().unwrap();
        let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
        let command = format!("& {{ $env:GJC_TEAM_WORKER = 'test/worker-1'; $env:GJC_TEAM_LEADER_CWD = {}; & {} 'test''s & prompt' }}",
            quote(path.parent().unwrap().to_str().unwrap()), quote(gjc));
        let worker = super::Worker::parse(&command, gjc).unwrap();
        assert_eq!(worker.prompt, "test's & prompt");
        assert!(super::Worker::parse(&command, "replaced.exe").is_none());
        assert!(super::Worker::parse(&command.replace("GJC_TEAM_WORKER", "PATH"), gjc).is_none());
        assert!(super::Worker::parse(&format!("{command}; & other"), gjc).is_none());
    }
}
