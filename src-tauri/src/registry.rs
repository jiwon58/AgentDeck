use crate::{
    adapter,
    model::{error, Agent, Candidate, Result},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

pub fn directory(path: &str) -> Result<PathBuf> {
    if !Path::new(path).is_absolute() {
        return Err("Select an absolute directory path".into());
    }
    let absolute = dunce::canonicalize(path).map_err(|e| format!("Directory unavailable: {e}"))?;
    if !absolute.is_dir() {
        return Err("Working path must be a directory".into());
    }
    Ok(absolute)
}

pub fn inspect(path: &str) -> Result<Candidate> {
    inspect_file(Path::new(path), false)
}

fn inspect_file(path: &Path, allow_script: bool) -> Result<Candidate> {
    if !Path::new(path).is_absolute() {
        return Err("Select an absolute executable path".into());
    }
    let absolute = dunce::canonicalize(path).map_err(|e| format!("Executable unavailable: {e}"))?;
    if !absolute.is_file() {
        return Err("Executable must be a file".into());
    }
    let requires_interpreter = script(&absolute);
    if (!allow_script && requires_interpreter)
        || (cfg!(windows)
            && !requires_interpreter
            && !absolute
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")))
    {
        return Err(
            "Use a native .exe or explicitly register its interpreter and arguments".into(),
        );
    }
    let mut file = File::open(&absolute).map_err(error)?;
    let architecture = if requires_interpreter {
        "script"
    } else {
        pe_machine(&mut file).unwrap_or("unknown")
    };
    file.rewind().map_err(error)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).map_err(error)?;
    Ok(Candidate {
        path: absolute
            .to_str()
            .ok_or("Executable path is not valid Unicode")?
            .into(),
        sha256: format!("{:x}", hash.finalize()),
        architecture: architecture.into(),
        launch_type: if requires_interpreter {
            "requires_interpreter"
        } else {
            "native_exe"
        }
        .into(),
    })
}

fn script(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        ["cmd", "bat", "ps1"]
            .iter()
            .any(|kind| ext.eq_ignore_ascii_case(kind))
    })
}

fn pe_machine(file: &mut File) -> Option<&'static str> {
    let mut header = [0; 64];
    file.read_exact(&mut header).ok()?;
    if &header[..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(header[60..64].try_into().ok()?);
    file.seek(SeekFrom::Start(offset.into())).ok()?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe).ok()?;
    if &pe[..4] != b"PE\0\0" {
        return None;
    }
    match u16::from_le_bytes([pe[4], pe[5]]) {
        0x8664 => Some("x64"),
        0x14c => Some("x86"),
        0xaa64 => Some("arm64"),
        _ => None,
    }
}

pub fn verify(candidate: &Candidate) -> Result<Candidate> {
    let actual = inspect(&candidate.path)?;
    if actual.sha256 != candidate.sha256 || actual.path != candidate.path {
        crate::diagnostics::warn("registry.executable_changed", None);
        return Err("Executable changed. Inspect and approve it again".into());
    }
    Ok(actual)
}

#[derive(serde::Serialize)]
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
}

pub fn scan(agent: Agent, path: &str) -> Result<Scan> {
    let root = directory(path)?;
    let mut result = Scan {
        candidates: Vec::new(),
        warnings: Vec::new(),
    };
    if root.to_string_lossy().starts_with("\\\\") {
        result
            .warnings
            .push("Network path: check access and file integrity before approval".into());
    }
    let mut dirs = vec![(root, 0)];
    let mut seen = std::collections::HashSet::new();
    let mut visited = 0;
    while let Some((dir, depth)) = dirs.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                result.warnings.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            visited += 1;
            if visited > 20_000 || result.candidates.len() >= 100 {
                result.warnings.push("Scan limit reached".into());
                return Ok(result);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    result.warnings.push(error(e));
                    continue;
                }
            };
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(value) => value,
                Err(e) => {
                    result.warnings.push(error(e));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if metadata.is_dir() && depth < 5 {
                dirs.push((entry.path(), depth + 1));
            }
            let path = entry.path();
            let native = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                || (!cfg!(windows) && path.extension().is_none() && agent != Agent::Generic);
            if metadata.is_file()
                && (native || script(&path))
                && path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| adapter::for_agent(agent).recognizes(name))
            {
                match inspect_file(&path, true) {
                    Ok(candidate) if seen.insert(candidate.path.to_lowercase()) => {
                        result.candidates.push(candidate)
                    }
                    Ok(_) => {}
                    Err(e) => result.warnings.push(e),
                }
            }
        }
    }
    result
        .candidates
        .sort_by(|a, b| a.launch_type.cmp(&b.launch_type).then(a.path.cmp(&b.path)));
    Ok(result)
}

pub fn version(candidate: &Candidate, args: &[String]) -> Result<Option<String>> {
    let actual = verify(candidate)?;
    let mut command = Command::new(&actual.path);
    command
        .args(args)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let job = crate::process::ProcessTree::new()?;
    let mut child = command.spawn().map_err(error)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        if let Err(e) = job.attach(child.as_raw_handle() as _) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    }
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0; 2048];
            while let Ok(count) = pipe.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                output.extend_from_slice(
                    &buffer[..count.min(8192_usize.saturating_sub(output.len()))],
                );
            }
            output
        })
    };
    let stdout = child.stdout.take().map(|p| drain(Box::new(p)));
    let stderr = child.stderr.take().map(|p| drain(Box::new(p)));
    let status = child.wait_timeout(Duration::from_secs(4)).map_err(error)?;
    if status.is_none() {
        crate::diagnostics::warn("registry.version_timeout", None);
        let _ = child.kill();
        let _ = child.wait();
    }
    drop(job);
    if !status.is_some_and(|s| s.success()) {
        if let Some(status) = status {
            crate::diagnostics::record(
                "WARN",
                "registry.version_process_failed",
                None,
                status.code().map(i64::from),
                None,
            );
        }
        return Ok(None);
    }
    // Do not wait indefinitely for an escaped descendant holding a pipe.
    let deadline = std::time::Instant::now() + Duration::from_millis(400);
    let handles: Vec<_> = [stdout, stderr].into_iter().flatten().collect();
    while handles.iter().any(|handle| !handle.is_finished()) && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let output: Vec<_> = handles
        .into_iter()
        .filter(|handle| handle.is_finished())
        .filter_map(|handle| handle.join().ok())
        .collect();
    Ok(output.iter().find_map(|bytes| {
        String::from_utf8_lossy(bytes)
            .split_whitespace()
            .find_map(|token| {
                let token = token
                    .trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '[' | ']'))
                    .rsplit('/')
                    .next()?
                    .trim_start_matches('v');
                let core = token.split(['-', '+']).next()?;
                let parts: Vec<_> = core.split('.').collect();
                (token.len() <= 100
                    && parts.len() == 3
                    && parts
                        .iter()
                        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
                .then(|| token.to_string())
            })
    }))
}
