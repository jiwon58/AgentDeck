use sha2::{Digest, Sha256};
use std::path::Path;

pub fn legacy(home: &str, temp: &str, cwd: &str, windows: bool) -> String {
    let normalize = |path: &str| {
        if windows {
            path.replace('/', "\\").trim_end_matches('\\').to_owned()
        } else {
            path.trim_end_matches('/').to_owned()
        }
    };
    let cwd = normalize(cwd);
    let separator = if windows { '\\' } else { '/' };
    let relative = |root: &str| {
        let root = normalize(root);
        let prefix = cwd.get(..root.len())?;
        let matches = if windows {
            prefix.to_lowercase() == root.to_lowercase()
        } else {
            prefix == root
        };
        if !matches {
            return None;
        }
        let suffix = &cwd[root.len()..];
        if suffix.is_empty() {
            Some(suffix)
        } else {
            suffix.strip_prefix(separator)
        }
    };
    let encode = |value: &str| value.replace(['/', '\\', ':'], "-");
    if let Some(suffix) = relative(home) {
        return format!("-{}", encode(suffix));
    }
    if !temp.is_empty() {
        if let Some(suffix) = relative(temp) {
            return if suffix.is_empty() {
                "-tmp".into()
            } else {
                format!("-tmp-{}", encode(suffix))
            };
        }
    }
    format!(
        "--{}--",
        encode(cwd.strip_prefix(separator).unwrap_or(&cwd))
    )
}

pub fn legacy_current(cwd: &str) -> Option<String> {
    let home = dunce::canonicalize(std::env::home_dir()?).ok()?;
    let temp = dunce::canonicalize(std::env::temp_dir()).ok()?;
    Some(legacy(home.to_str()?, temp.to_str()?, cwd, cfg!(windows)))
}

pub fn v2_digest(platform: &str, identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gjc-managed-session-scope\0identity-v1\0");
    hasher.update(platform.as_bytes());
    hasher.update(b"\0");
    hasher.update(identity.as_bytes());
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut name = String::from("v2-");
    let (mut accumulator, mut bits) = (0u32, 0);
    for byte in hasher.finalize() {
        accumulator = (accumulator << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            name.push(char::from(alphabet[((accumulator >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        name.push(char::from(
            alphabet[((accumulator << (5 - bits)) & 31) as usize],
        ));
    }
    name
}

#[cfg(windows)]
pub fn v2_identity(cwd: &Path) -> Option<String> {
    use std::{
        fs::OpenOptions,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFinalPathNameByHandleW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        VOLUME_NAME_GUID,
    };

    if !cwd.is_absolute() {
        return None;
    }
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(cwd)
        .ok()?;
    if !file.metadata().ok()?.is_dir() {
        return None;
    }
    let mut buffer = vec![0; 32_768];
    // The open directory handle remains alive and the writable UTF-16 buffer has the supplied length.
    let len = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    if len == 0 || len >= buffer.len() {
        return None;
    }
    let value = String::from_utf16(&buffer[..len]).ok()?;
    value.starts_with("\\\\?\\Volume{").then_some(value)
}

#[cfg(not(windows))]
pub fn v2_identity(cwd: &Path) -> Option<String> {
    if !cwd.is_dir() {
        return None;
    }
    std::fs::canonicalize(cwd)
        .ok()?
        .into_os_string()
        .into_string()
        .ok()
}
