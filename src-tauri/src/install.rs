use crate::{
    model::{error, Result},
    registry,
};
use reqwest::blocking::{Client, Response};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const CLAUDE: &str = "https://downloads.claude.ai/claude-code-releases";
const HERMES: &str =
    "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1";

#[derive(Serialize)]
pub struct Download {
    pub path: String,
    pub sha256: String,
    pub version: Option<String>,
}

pub struct HermesReceipt {
    path: PathBuf,
    payload: Vec<u8>,
    sha256: String,
}

impl HermesReceipt {
    fn reviewed(&self, reviewed_sha256: &str) -> Result<()> {
        let metadata = self.path.symlink_metadata().map_err(error)?;
        if self.sha256 != reviewed_sha256
            || metadata.len() != self.payload.len() as u64
            || !metadata.file_type().is_file()
            || fs::read(&self.path).map_err(error)? != self.payload
        {
            return Err(
                "The downloaded installer changed. Download it again before installing.".into(),
            );
        }
        Ok(())
    }
}

fn client() -> Result<Client> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(1200))
        .build()
        .map_err(error)
}

fn get(client: &Client, url: &str) -> Result<Response> {
    let response = client.get(url).send().map_err(error)?;
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(format!(
            "Publisher download returned HTTP {}",
            response.status().as_u16()
        ))
    }
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn small(response: Response, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > limit {
        return Err("Publisher response is too large".into());
    }
    Ok(bytes)
}

fn digest_matches(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
}

fn save(target: &Path, response: impl Read, limit: u64, expected: &str) -> Result<()> {
    let parent = target.parent().ok_or("Download path has no parent")?;
    let mut output = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    let mut input = response.take(limit + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        if input.limit() == 0 {
            return Err("Publisher payload is too large".into());
        }
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(error)?;
    }
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected) {
        return Err("Publisher checksum did not match the download".into());
    }
    output.as_file().sync_all().map_err(error)?;
    output.persist_noclobber(target).map_err(|err| {
        if err.error.kind() == std::io::ErrorKind::AlreadyExists {
            "A file already exists at the destination. Select an empty folder to avoid replacing it.".into()
        } else { error(err) }
    })?;
    Ok(())
}

pub fn claude(directory: &str) -> Result<Download> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("Claude download is available on Windows x64".into());
    }
    let directory = registry::directory(directory)?;
    let path = directory.join("claude.exe");
    if path.symlink_metadata().is_ok() {
        return Err("claude.exe already exists. Select a different folder.".into());
    }
    let client = client()?;
    let version = String::from_utf8(small(get(&client, &format!("{CLAUDE}/latest"))?, 128)?)
        .map_err(error)?;
    let version = version.trim();
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|part| part.parse::<u32>().is_ok() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("The Claude release channel returned an invalid version".into());
    }
    let manifest = small(
        get(&client, &format!("{CLAUDE}/{version}/manifest.json"))?,
        256 * 1024,
    )?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).map_err(error)?;
    let sha256 = manifest["platforms"]["win32-x64"]["checksum"]
        .as_str()
        .filter(|value| digest_matches(value))
        .ok_or("No valid Windows x64 checksum in the Claude release manifest")?;
    let binary = get(&client, &format!("{CLAUDE}/{version}/win32-x64/claude.exe"))?;
    save(&path, binary, 768 * 1024 * 1024, sha256)?;
    Ok(Download {
        path: path.to_string_lossy().into_owned(),
        sha256: sha256.to_lowercase(),
        version: Some(version.into()),
    })
}

pub fn hermes(directory: &str) -> Result<(Download, HermesReceipt)> {
    if !cfg!(windows) {
        return Err("Hermes installation is available on Windows".into());
    }
    let directory = registry::directory(directory)?;
    let path = directory.join("hermes-install.ps1");
    let payload = small(get(&client()?, HERMES)?, 2 * 1024 * 1024)?;
    let text = std::str::from_utf8(&payload).map_err(error)?;
    if !["param(", "$HermesHome", "$InstallDir", "$NonInteractive"]
        .iter()
        .all(|tag| text.contains(tag))
    {
        return Err(
            "The Hermes installer did not match the documented Windows bootstrap interface".into(),
        );
    }
    let sha256 = sha(&payload);
    if path.symlink_metadata().is_ok() {
        let receipt = HermesReceipt {
            path: path.clone(),
            payload: payload.clone(),
            sha256: sha256.clone(),
        };
        receipt.reviewed(&sha256)?;
    } else {
        save(&path, &payload[..], 2 * 1024 * 1024, &sha256)?;
    }
    Ok((
        Download {
            path: path.to_string_lossy().into_owned(),
            sha256: sha256.clone(),
            version: None,
        },
        HermesReceipt {
            path,
            payload,
            sha256,
        },
    ))
}

#[cfg(windows)]
pub fn install_hermes(receipt: &HermesReceipt, reviewed_sha256: &str) -> Result<registry::Scan> {
    use std::{
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Command, Stdio},
    };
    use wait_timeout::ChildExt;
    receipt.reviewed(reviewed_sha256)?;
    let root = receipt.path.parent().ok_or("Missing selected directory")?;
    if registry::directory(
        root.to_str()
            .ok_or("Destination path is not valid Unicode")?,
    )? != root
    {
        return Err(
            "The selected installation directory now resolves to a different location".into(),
        );
    }
    let home = root.join("hermes-home");
    let checkout = root.join("hermes-agent");
    if [&home, &checkout]
        .into_iter()
        .any(|path| path.symlink_metadata().is_ok())
    {
        return Err(
            "Hermes home or checkout already exists. Select a new folder for a fresh installation."
                .into(),
        );
    }
    let mut script = tempfile::Builder::new()
        .prefix("deck-hermes-")
        .suffix(".ps1")
        .tempfile()
        .map_err(error)?;
    script.write_all(&receipt.payload).map_err(error)?;
    let script = script.into_temp_path();
    let powershell = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Windows SystemRoot is unavailable")?
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let tree = crate::process::ProcessTree::new()?;
    let mut child = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "RemoteSigned",
            "-File",
        ])
        .arg(&script)
        .arg("-HermesHome")
        .arg(&home)
        .arg("-InstallDir")
        .arg(&checkout)
        .args(["-NonInteractive", "-Channel", "stable"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(error)?;
    if let Err(e) = tree.attach(child.as_raw_handle() as _) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }
    let Some(status) = child
        .wait_timeout(Duration::from_secs(30 * 60))
        .map_err(error)?
    else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("The Hermes installer timed out. Inspect the selected directory for partial installation files.".into());
    };
    if !status.success() {
        return Err(format!("The Hermes installer exited with {}. Check the vendor's Windows requirements and the selected folder.", status.code().unwrap_or(-1)));
    }
    let launcher = home.join("bin").join("hermes.exe");
    if launcher.is_file() {
        return Ok(registry::Scan {
            candidates: vec![registry::inspect(
                launcher
                    .to_str()
                    .ok_or("Hermes launcher path is not valid Unicode")?,
            )?],
            warnings: Vec::new(),
        });
    }
    // Other publisher layouts can include an interpreter launcher in the checkout.
    let mut result = registry::scan(crate::model::Agent::Hermes, &home.to_string_lossy())?;
    if result.candidates.is_empty() {
        result = registry::scan(crate::model::Agent::Hermes, &checkout.to_string_lossy())?;
    }
    Ok(result)
}

#[cfg(not(windows))]
pub fn install_hermes(_receipt: &HermesReceipt, _reviewed_sha256: &str) -> Result<registry::Scan> {
    Err("Hermes installation is available on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_are_atomic_and_keep_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("binary");
        assert!(save(&path, &b"data"[..], 16, &sha(b"other")).is_err());
        assert!(!path.exists());
        assert!(save(&path, &b"large"[..], 2, &sha(b"large")).is_err());
        assert!(!path.exists());
        save(&path, &b"data"[..], 16, &sha(b"data")).unwrap();
        assert!(save(&path, &b"other"[..], 16, &sha(b"other")).is_err());
        assert_eq!(fs::read(path).unwrap(), b"data");
    }

    #[test]
    fn altered_installer_or_receipt_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hermes-install.ps1");
        fs::write(&path, b"original").unwrap();
        let receipt = HermesReceipt {
            path,
            payload: b"original".to_vec(),
            sha256: sha(b"original"),
        };
        assert!(receipt.reviewed(&sha(b"other")).is_err());
        assert!(receipt.reviewed(&receipt.sha256).is_ok());
        fs::write(&receipt.path, b"modified").unwrap();
        assert!(receipt.reviewed(&receipt.sha256).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_installer_passes_selected_paths_and_finds_launcher() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("한글 folder");
        fs::create_dir(&root).unwrap();
        let root = dunce::canonicalize(root).unwrap();
        let path = root.join("hermes-install.ps1");
        let payload = br#"
param([string]$HermesHome, [string]$InstallDir, [switch]$NonInteractive, [string]$Channel)
$ErrorActionPreference = "Stop"
if (!$NonInteractive -or $Channel -ne "stable") { exit 7 }
New-Item -ItemType Directory -Path $InstallDir | Out-Null
$bin = Join-Path $HermesHome "bin"
New-Item -ItemType Directory -Path $bin -Force | Out-Null
Copy-Item (Join-Path $env:SystemRoot "System32\whoami.exe") (Join-Path $bin "hermes.exe")
"#
        .to_vec();
        fs::write(&path, &payload).unwrap();
        let receipt = HermesReceipt {
            path,
            sha256: sha(&payload),
            payload,
        };
        let result = install_hermes(&receipt, &receipt.sha256).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            Path::new(&result.candidates[0].path),
            root.join("hermes-home/bin/hermes.exe")
        );
        assert!(root.join("hermes-agent").is_dir());
    }
}
