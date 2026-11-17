use crate::{
    model::{error, Agent, Result, Runtime},
    registry,
};
use portable_pty::CommandBuilder;
use serde::Serialize;
use std::path::{Path, PathBuf};

const SHA256: &str = "bbe825a30f7cddc91b043084049143a381aba5f7e9e36e4d0bb587e296d262ca";

fn backend() -> Option<PathBuf> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return None;
    }
    #[cfg(feature = "test-fixture")]
    if let Some(path) = std::env::var_os("DECK_TEAM_PSMUX") {
        return Some(path.into());
    }
    Some(
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("team/psmux.exe"),
    )
}

pub fn available() -> bool {
    backend().is_some_and(|path| path.is_file() && bridge_path(&path).is_file())
}

fn bridge_path(engine: &Path) -> PathBuf {
    engine.with_file_name("deck-team-mux.exe")
}

#[derive(Serialize)]
pub struct Plan {
    pub executable: String,
    pub sha256: &'static str,
    pub bridge: String,
    #[serde(skip)]
    gjc_sha256: String,
}

impl Plan {
    pub fn verify(runtime: &Runtime) -> Result<Self> {
        if runtime.agent != Agent::Gajae
            || !runtime.trusted
            || !runtime.verified
            || runtime.version != "0.10.2"
            || !runtime.executable.architecture.eq_ignore_ascii_case("x64")
        {
            return Err("Native Team requires a verified Gajae-Code 0.10.2 x64 runtime".into());
        }
        let path = backend()
            .filter(|path| path.is_file())
            .ok_or("Native Team requires the Agent Deck package that includes psmux")?;
        let candidate = registry::inspect(path.to_str().ok_or("Invalid native Team engine path")?)?;
        if candidate.sha256 != SHA256 {
            return Err("Native Team engine hash mismatch. Check the portable package.".into());
        }
        let bridge = bridge_path(Path::new(&candidate.path));
        if !bridge.is_file() {
            return Err(
                "Native Team compatibility helper is missing. Check the portable package.".into(),
            );
        }
        let bridge = bridge.to_str().ok_or("Invalid native Team helper path")?;
        // psmux 3.4.0 detects shell syntax in respawn argv.
        if bridge.contains(['&', '|', '<', '>', ';', '`', '$', '(', ')', '%', '\r', '\n']) {
            return Err("Extract the Team package to a folder without shell metacharacters".into());
        }
        Ok(Self {
            executable: candidate.path,
            sha256: SHA256,
            bridge: bridge.to_owned(),
            gjc_sha256: runtime.executable.sha256.clone(),
        })
    }

    pub fn prepare(
        self,
        app_data: &Path,
        id: &str,
        command: &mut CommandBuilder,
    ) -> Result<Workspace> {
        let argv = command.get_argv();
        let gjc = argv
            .first()
            .and_then(|arg| arg.to_str())
            .ok_or("Missing GJC executable")?;
        let worker = worker_command(gjc)?;
        let gjc = gjc.to_owned();
        if argv.iter().skip(1).any(|argument| {
            let arg = argument.to_string_lossy();
            let flag = arg.split('=').next().unwrap_or_default();
            matches!(flag, "--tmux" | "--no-tmux" | "--print" | "-p" | "--mode")
        }) {
            return Err(
                "Native Team needs an interactive GJC launch without tmux or print flags".into(),
            );
        }
        let cwd = command
            .get_cwd()
            .ok_or("Missing GJC working directory")?
            .clone();
        let name = format!("deck-{id}");
        let directory = tempfile::Builder::new()
            .prefix("team-")
            .tempdir_in(app_data)
            .map_err(error)?;
        // The stock Team skill and worker prompts also call `gjc team api`.
        // Resolve those calls inside this session to the selected runtime.
        let alias = directory.path().join("gjc.exe");
        if std::fs::hard_link(&gjc, &alias).is_err() {
            std::fs::copy(&gjc, &alias).map_err(error)?;
        }
        let previous_path = command.get_env("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(directory.path().to_owned())
                .chain(std::env::split_paths(previous_path)),
        )
        .map_err(error)?;
        let config = directory.path().join("psmux.conf");
        std::fs::write(&config, "set -g mouse on\n").map_err(error)?;
        let mut prefix = [self.executable.as_str(), "new-session", "-s", &name, "-c"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        prefix.extend([cwd, "--".into()]);
        command.get_argv_mut().splice(..0, prefix);
        let reserved: Vec<_> = command
            .iter_full_env_as_str()
            .filter(|(key, _)| reserved_environment(key))
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in reserved {
            command.env_remove(key);
        }
        command.env("PATH", path);
        command.env("PSMUX_DATA_DIR", directory.path());
        command.env("PSMUX_CONFIG_FILE", config);
        // Cold startup inherits the process environment without writing out its values.
        command.env("PSMUX_NO_WARM", "1");
        command.env("PSMUX_PSREADLINE_HISTORY", "NUL");
        command.env("GJC_TMUX_COMMAND", &self.bridge);
        command.env("GJC_TEAM_TMUX_COMMAND", &self.bridge);
        command.env("GJC_TMUX_ACTIVE_SESSION", name);
        command.env("GJC_TEAM_WORKER_COMMAND", worker);
        command.env("DECK_TEAM_GJC", gjc);
        command.env("DECK_TEAM_GJC_SHA256", self.gjc_sha256);
        Ok(Workspace {
            engine: self.executable,
            directory,
        })
    }
}

fn reserved_environment(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    key.starts_with("PSMUX_")
        || key.starts_with("TMUX")
        || key.starts_with("GJC_TMUX_")
        || key.starts_with("GJC_TEAM_")
        || matches!(key.as_str(), "DECK_TEAM_GJC" | "DECK_TEAM_GJC_SHA256")
}

fn worker_command(path: &str) -> Result<String> {
    if path.contains(['\r', '\n']) {
        return Err("Invalid character in the GJC executable path".into());
    }
    // GJC 0.10.2 quotes arguments for PowerShell. The helper decodes this
    // literal and passes the original path directly to the pane process.
    let path = path.replace('\'', "''");
    Ok(format!("'{path}'"))
}

pub struct Workspace {
    engine: String,
    directory: tempfile::TempDir,
}

impl Drop for Workspace {
    fn drop(&mut self) {
        use std::process::{Command, Stdio};
        use wait_timeout::ChildExt;

        if !registry::inspect(&self.engine).is_ok_and(|candidate| candidate.sha256 == SHA256) {
            return;
        }
        let mut command = Command::new(&self.engine);
        command
            .args(["kill-server"])
            .env_clear()
            .env("PSMUX_DATA_DIR", self.directory.path())
            .env("PSMUX_NO_WARM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
        }
        if let Ok(mut child) = command.spawn() {
            if !matches!(
                child.wait_timeout(std::time::Duration::from_millis(750)),
                Ok(Some(_))
            ) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn worker_path_is_a_powershell_literal() {
        assert_eq!(
            super::worker_command(r"C:\한 글\O'Brien & (team)\gjc.exe").unwrap(),
            r"'C:\한 글\O''Brien & (team)\gjc.exe'"
        );
        assert!(super::worker_command("a\rb").is_err());
    }
}
