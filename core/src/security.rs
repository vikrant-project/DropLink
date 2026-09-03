use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};

/// Windows reserved device names that cannot be used as filenames.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitizes a raw untrusted filename from a peer, stripping directory traversals,
/// reserved names, invalid characters, and control characters.
pub fn sanitize_filename(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());

    for ch in raw.chars() {
        match ch {
            // Replace path separators and illegal Windows/POSIX characters
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => {
                cleaned.push('_');
            }
            // Strip non-printable ASCII and control characters
            c if c.is_control() => {
                cleaned.push('_');
            }
            c => cleaned.push(c),
        }
    }

    // Trim leading and trailing whitespace and dots (illegal on Windows filesystems)
    let mut trimmed = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.').to_string();

    // Strip any double-dot sequences (e.g. .., ..., ....)
    while trimmed.contains("..") {
        trimmed = trimmed.replace("..", "_");
    }

    trimmed = trimmed.trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '_').to_string();

    if trimmed.is_empty() {
        trimmed = "unnamed_file".to_string();
    }

    // Check against Windows reserved names (case-insensitive base stem)
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or(&trimmed)
        .to_uppercase();

    if RESERVED_NAMES.contains(&stem.as_str()) {
        trimmed = format!("_{}", trimmed);
    }

    // Truncate filename if it exceeds standard filesystem limit (255 chars)
    if trimmed.len() > 250 {
        if let Some(dot_idx) = trimmed.rfind('.') {
            let ext = &trimmed[dot_idx..];
            let max_base = 250 - ext.len();
            trimmed = format!("{}{}", &trimmed[..max_base], ext);
        } else {
            trimmed.truncate(250);
        }
    }

    trimmed
}

/// Resolves a safe destination path strictly confined within the target root directory.
/// Prevents path traversal vulnerabilities (`../`, absolute paths, symlink escapes).
pub fn resolve_safe_path(base_dir: &Path, raw_name: &str) -> Result<PathBuf> {
    let clean_name = sanitize_filename(raw_name);
    let candidate = base_dir.join(&clean_name);

    // Verify candidate does not contain ParentDir components
    for comp in candidate.components() {
        if let Component::ParentDir = comp {
            bail!("Path traversal attempt detected in filename: {:?}", raw_name);
        }
    }

    Ok(candidate)
}

/// Computes an auto-renamed path if the destination already exists.
/// Example: `photo.jpg` -> `photo (1).jpg` -> `photo (2).jpg`
pub fn resolve_conflict_path(target_path: &Path) -> PathBuf {
    if !target_path.exists() {
        return target_path.to_path_buf();
    }

    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = target_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = target_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();

    for i in 1..10_000 {
        let candidate_name = format!("{} ({}){}", stem, i, ext);
        let candidate_path = parent.join(candidate_name);
        if !candidate_path.exists() {
            return candidate_path;
        }
    }

    // Fallback: timestamp suffix
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    parent.join(format!("{}_{}{}", stem, ts, ext))
}
