//! Typed shell-like workspace operations shared between the native
//! host and the wasm guest. The API is intentionally structured:
//! callers send a tagged enum, not raw `sh -c` text, so unsupported
//! shell syntax is rejected by construction.

use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{SearchError, SearchMatch, search_file_contents};

pub const TOOLBOX_DEFAULT_MAX_READ_BYTES: u64 = 1024 * 1024;
pub const TOOLBOX_DEFAULT_MAX_LIST_ENTRIES: usize = 1024;
pub const TOOLBOX_DEFAULT_MAX_FIND_RESULTS: usize = 1024;
pub const TOOLBOX_DEFAULT_MAX_COPY_BYTES: u64 = 64 * 1024 * 1024;
pub const TOOLBOX_DEFAULT_MAX_MUTATION_ENTRIES: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum WorkspaceCommand {
    Pwd,
    Ls {
        path: Option<String>,
        recursive: bool,
        max_entries: Option<usize>,
    },
    Cat {
        path: String,
        max_bytes: Option<u64>,
    },
    Head {
        path: String,
        lines: usize,
        max_bytes: Option<u64>,
    },
    Tail {
        path: String,
        lines: usize,
        max_bytes: Option<u64>,
    },
    Wc {
        path: String,
        max_bytes: Option<u64>,
    },
    Find {
        path: Option<String>,
        glob: Option<String>,
        max_results: Option<usize>,
    },
    Grep {
        path: Option<String>,
        pattern: String,
        glob: Option<String>,
        max_results: usize,
        max_file_bytes: u64,
        max_total_bytes: u64,
    },
    Mkdir {
        path: String,
        parents: bool,
    },
    Cp {
        source: String,
        destination: String,
        recursive: bool,
        overwrite: bool,
        max_total_bytes: Option<u64>,
        max_entries: Option<usize>,
    },
    Mv {
        source: String,
        destination: String,
        overwrite: bool,
    },
    Rm {
        path: String,
        recursive: bool,
        max_entries: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub path: String,
    pub kind: WorkspaceEntryKind,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WordCount {
    pub bytes: u64,
    pub chars: usize,
    pub words: usize,
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WorkspaceCommandOutput {
    Pwd {
        path: String,
    },
    Ls {
        entries: Vec<WorkspaceEntry>,
        truncated: bool,
    },
    Cat {
        content: String,
    },
    Head {
        content: String,
    },
    Tail {
        content: String,
    },
    Wc {
        counts: WordCount,
    },
    Find {
        paths: Vec<String>,
        truncated: bool,
    },
    Grep {
        matches: Vec<SearchMatch>,
        truncated: bool,
    },
    Mutation {
        action: String,
        path: String,
    },
}

#[derive(Debug)]
pub enum ToolboxError {
    InvalidPath(String),
    PathEscapesWorkspace(String),
    NotFound(String),
    NotAFile(String),
    NotADirectory(String),
    TooLarge(String),
    AlreadyExists(String),
    RecursiveRequired(String),
    InvalidArgument(String),
    InvalidGlob(String),
    Io(String),
    Search(SearchError),
}

impl std::fmt::Display for ToolboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(s) => write!(f, "Invalid path: {s}"),
            Self::PathEscapesWorkspace(s) => write!(f, "Path escapes workspace: {s}"),
            Self::NotFound(s) => write!(f, "Path not found: {s}"),
            Self::NotAFile(s) => write!(f, "Not a regular file: {s}"),
            Self::NotADirectory(s) => write!(f, "Not a directory: {s}"),
            Self::TooLarge(s) => write!(f, "Operation exceeds size budget: {s}"),
            Self::AlreadyExists(s) => write!(f, "Path already exists: {s}"),
            Self::RecursiveRequired(s) => write!(f, "Recursive flag required: {s}"),
            Self::InvalidArgument(s) => write!(f, "Invalid argument: {s}"),
            Self::InvalidGlob(s) => write!(f, "Invalid glob: {s}"),
            Self::Io(s) => write!(f, "I/O error: {s}"),
            Self::Search(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ToolboxError {}

impl From<SearchError> for ToolboxError {
    fn from(value: SearchError) -> Self {
        match value {
            SearchError::InvalidGlob(msg) => Self::InvalidGlob(msg),
            other => Self::Search(other),
        }
    }
}

pub fn run_workspace_command(
    root: &Path,
    command: &WorkspaceCommand,
) -> Result<WorkspaceCommandOutput, ToolboxError> {
    let root = canonical_root(root)?;
    match command {
        WorkspaceCommand::Pwd => Ok(WorkspaceCommandOutput::Pwd {
            path: path_to_display(&root, &root),
        }),
        WorkspaceCommand::Ls {
            path,
            recursive,
            max_entries,
        } => {
            let path = resolve_existing_path(&root, path.as_deref(), PathExpectation::Any)?;
            let (entries, truncated) = list_directory(
                &root,
                &path,
                *recursive,
                max_entries.unwrap_or(TOOLBOX_DEFAULT_MAX_LIST_ENTRIES),
            )?;
            Ok(WorkspaceCommandOutput::Ls { entries, truncated })
        }
        WorkspaceCommand::Cat { path, max_bytes } => {
            let content = read_text_file(
                &root,
                path,
                max_bytes.unwrap_or(TOOLBOX_DEFAULT_MAX_READ_BYTES),
            )?;
            Ok(WorkspaceCommandOutput::Cat { content })
        }
        WorkspaceCommand::Head {
            path,
            lines,
            max_bytes,
        } => {
            let content = read_text_file(
                &root,
                path,
                max_bytes.unwrap_or(TOOLBOX_DEFAULT_MAX_READ_BYTES),
            )?;
            Ok(WorkspaceCommandOutput::Head {
                content: head_lines(&content, *lines),
            })
        }
        WorkspaceCommand::Tail {
            path,
            lines,
            max_bytes,
        } => {
            let content = read_text_file(
                &root,
                path,
                max_bytes.unwrap_or(TOOLBOX_DEFAULT_MAX_READ_BYTES),
            )?;
            Ok(WorkspaceCommandOutput::Tail {
                content: tail_lines(&content, *lines),
            })
        }
        WorkspaceCommand::Wc { path, max_bytes } => {
            let content = read_text_file(
                &root,
                path,
                max_bytes.unwrap_or(TOOLBOX_DEFAULT_MAX_READ_BYTES),
            )?;
            Ok(WorkspaceCommandOutput::Wc {
                counts: WordCount {
                    bytes: content.len() as u64,
                    chars: content.chars().count(),
                    words: content.split_whitespace().count(),
                    lines: content.as_bytes().iter().filter(|&&b| b == b'\n').count(),
                },
            })
        }
        WorkspaceCommand::Find {
            path,
            glob,
            max_results,
        } => {
            let path = resolve_existing_path(&root, path.as_deref(), PathExpectation::Any)?;
            let (paths, truncated) = find_paths(
                &root,
                &path,
                glob.as_deref(),
                max_results.unwrap_or(TOOLBOX_DEFAULT_MAX_FIND_RESULTS),
            )?;
            Ok(WorkspaceCommandOutput::Find { paths, truncated })
        }
        WorkspaceCommand::Grep {
            path,
            pattern,
            glob,
            max_results,
            max_file_bytes,
            max_total_bytes,
        } => {
            let target = resolve_existing_path(&root, path.as_deref(), PathExpectation::Any)?;
            let outcome = grep_path(
                &root,
                &target,
                pattern,
                glob.as_deref(),
                *max_results,
                *max_file_bytes,
                *max_total_bytes,
            )?;
            Ok(WorkspaceCommandOutput::Grep {
                matches: outcome.matches,
                truncated: outcome.truncated,
            })
        }
        WorkspaceCommand::Mkdir { path, parents } => {
            let target = resolve_new_path(&root, path, true)?;
            if target.exists() {
                return Err(ToolboxError::AlreadyExists(path.clone()));
            }
            if *parents {
                std::fs::create_dir_all(&target).map_err(io_error)?;
            } else {
                std::fs::create_dir(&target).map_err(io_error)?;
            }
            Ok(WorkspaceCommandOutput::Mutation {
                action: "mkdir".to_string(),
                path: path_to_display(&root, &target),
            })
        }
        WorkspaceCommand::Cp {
            source,
            destination,
            recursive,
            overwrite,
            max_total_bytes,
            max_entries,
        } => {
            let source_path = resolve_existing_path(&root, Some(source), PathExpectation::Any)?;
            let destination_path = resolve_copy_move_destination(&root, &source_path, destination)?;
            copy_path(
                &root,
                &source_path,
                &destination_path,
                *recursive,
                *overwrite,
                max_total_bytes.unwrap_or(TOOLBOX_DEFAULT_MAX_COPY_BYTES),
                max_entries.unwrap_or(TOOLBOX_DEFAULT_MAX_MUTATION_ENTRIES),
            )?;
            Ok(WorkspaceCommandOutput::Mutation {
                action: "cp".to_string(),
                path: path_to_display(&root, &destination_path),
            })
        }
        WorkspaceCommand::Mv {
            source,
            destination,
            overwrite,
        } => {
            let source_path = resolve_existing_path(&root, Some(source), PathExpectation::Any)?;
            let destination_path = resolve_copy_move_destination(&root, &source_path, destination)?;
            move_path(&root, &source_path, &destination_path, *overwrite)?;
            Ok(WorkspaceCommandOutput::Mutation {
                action: "mv".to_string(),
                path: path_to_display(&root, &destination_path),
            })
        }
        WorkspaceCommand::Rm {
            path,
            recursive,
            max_entries,
        } => {
            let target = resolve_existing_path(&root, Some(path), PathExpectation::Any)?;
            if target == root {
                return Err(ToolboxError::InvalidArgument(
                    "refusing to remove workspace root".to_string(),
                ));
            }
            remove_path(
                &target,
                *recursive,
                max_entries.unwrap_or(TOOLBOX_DEFAULT_MAX_MUTATION_ENTRIES),
            )?;
            Ok(WorkspaceCommandOutput::Mutation {
                action: "rm".to_string(),
                path: path_to_display(&root, &target),
            })
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PathExpectation {
    Any,
    File,
}

fn canonical_root(root: &Path) -> Result<PathBuf, ToolboxError> {
    let canonical = root.canonicalize().map_err(io_error)?;
    if !canonical.is_dir() {
        return Err(ToolboxError::NotADirectory(root.display().to_string()));
    }
    Ok(canonical)
}

fn resolve_existing_path(
    root: &Path,
    input: Option<&str>,
    expectation: PathExpectation,
) -> Result<PathBuf, ToolboxError> {
    let relative = normalize_relative_path(input.unwrap_or("."), true)?;
    let candidate = root.join(&relative);
    let canonical = candidate.canonicalize().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ToolboxError::NotFound(input.unwrap_or(".").to_string())
        } else {
            io_error(err)
        }
    })?;
    if !canonical.starts_with(root) {
        return Err(ToolboxError::PathEscapesWorkspace(
            input.unwrap_or(".").to_string(),
        ));
    }
    match expectation {
        PathExpectation::File if !canonical.is_file() => {
            Err(ToolboxError::NotAFile(input.unwrap_or(".").to_string()))
        }
        _ => Ok(canonical),
    }
}

fn resolve_new_path(
    root: &Path,
    input: &str,
    must_have_parent: bool,
) -> Result<PathBuf, ToolboxError> {
    let relative = normalize_relative_path(input, false)?;
    if relative.as_os_str().is_empty() {
        return Err(ToolboxError::InvalidPath(input.to_string()));
    }
    let candidate = root.join(&relative);
    ensure_existing_ancestor_within_root(root, &candidate)?;
    if must_have_parent {
        let parent = candidate
            .parent()
            .ok_or_else(|| ToolboxError::InvalidPath(input.to_string()))?;
        if !parent.exists() {
            return Err(ToolboxError::NotFound(parent.display().to_string()));
        }
    }
    Ok(candidate)
}

fn ensure_existing_ancestor_within_root(root: &Path, path: &Path) -> Result<(), ToolboxError> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            let canonical = candidate.canonicalize().map_err(io_error)?;
            if canonical.starts_with(root) {
                return Ok(());
            }
            return Err(ToolboxError::PathEscapesWorkspace(
                path.display().to_string(),
            ));
        }
        current = candidate.parent();
    }
    Err(ToolboxError::PathEscapesWorkspace(
        path.display().to_string(),
    ))
}

fn normalize_relative_path(input: &str, allow_root: bool) -> Result<PathBuf, ToolboxError> {
    let raw = input.trim();
    if raw.is_empty() {
        if allow_root {
            return Ok(PathBuf::new());
        }
        return Err(ToolboxError::InvalidPath(input.to_string()));
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(ToolboxError::PathEscapesWorkspace(input.to_string()));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ToolboxError::InvalidPath(input.to_string()));
            }
        }
    }

    if !allow_root && normalized.as_os_str().is_empty() {
        return Err(ToolboxError::InvalidPath(input.to_string()));
    }
    Ok(normalized)
}

fn list_directory(
    root: &Path,
    target: &Path,
    recursive: bool,
    max_entries: usize,
) -> Result<(Vec<WorkspaceEntry>, bool), ToolboxError> {
    if target.is_file() {
        return Ok((vec![workspace_entry(root, target)?], false));
    }
    if !target.is_dir() {
        return Err(ToolboxError::NotADirectory(target.display().to_string()));
    }

    let mut entries = Vec::new();
    let mut truncated = false;
    if recursive {
        for entry in walkdir::WalkDir::new(target)
            .min_depth(1)
            .into_iter()
            .flatten()
        {
            entries.push(workspace_entry(root, entry.path())?);
            if entries.len() >= max_entries {
                truncated = true;
                break;
            }
        }
    } else {
        for entry in std::fs::read_dir(target).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            entries.push(workspace_entry(root, &entry.path())?);
            if entries.len() >= max_entries {
                truncated = true;
                break;
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((entries, truncated))
}

fn workspace_entry(root: &Path, path: &Path) -> Result<WorkspaceEntry, ToolboxError> {
    let meta = path.metadata().map_err(io_error)?;
    let kind = if meta.is_dir() {
        WorkspaceEntryKind::Directory
    } else if meta.is_file() {
        WorkspaceEntryKind::File
    } else {
        return Err(ToolboxError::InvalidArgument(format!(
            "unsupported entry type: {}",
            path.display()
        )));
    };
    Ok(WorkspaceEntry {
        path: path_to_display(root, path),
        kind: kind.clone(),
        size_bytes: match kind {
            WorkspaceEntryKind::File => Some(meta.len()),
            WorkspaceEntryKind::Directory => None,
        },
    })
}

fn read_text_file(root: &Path, path: &str, max_bytes: u64) -> Result<String, ToolboxError> {
    let target = resolve_existing_path(root, Some(path), PathExpectation::File)?;
    let meta = target.metadata().map_err(io_error)?;
    if meta.len() > max_bytes {
        return Err(ToolboxError::TooLarge(format!(
            "{path} is {} bytes and exceeds the {max_bytes}-byte limit",
            meta.len()
        )));
    }
    std::fs::read_to_string(&target).map_err(io_error)
}

fn head_lines(content: &str, lines: usize) -> String {
    if lines == 0 {
        return String::new();
    }
    content.split_inclusive('\n').take(lines).collect()
}

fn tail_lines(content: &str, lines: usize) -> String {
    if lines == 0 {
        return String::new();
    }
    let segments: Vec<&str> = content.split_inclusive('\n').collect();
    let start = segments.len().saturating_sub(lines);
    segments[start..].concat()
}

fn find_paths(
    root: &Path,
    target: &Path,
    glob: Option<&str>,
    max_results: usize,
) -> Result<(Vec<String>, bool), ToolboxError> {
    let glob_re = match glob {
        Some(pattern) => Some(compile_glob(pattern)?),
        None => None,
    };
    let mut paths = Vec::new();
    let mut truncated = false;

    if target.is_file() {
        let display = path_to_display(root, target);
        if glob_re.as_ref().is_none_or(|re| re.is_match(&display)) {
            paths.push(display);
        }
        return Ok((paths, false));
    }

    for entry in walkdir::WalkDir::new(target)
        .min_depth(1)
        .into_iter()
        .flatten()
    {
        let display = path_to_display(root, entry.path());
        if glob_re.as_ref().is_some_and(|re| !re.is_match(&display)) {
            continue;
        }
        paths.push(display);
        if paths.len() >= max_results {
            truncated = true;
            break;
        }
    }

    paths.sort();
    Ok((paths, truncated))
}

fn grep_path(
    root: &Path,
    target: &Path,
    pattern: &str,
    glob: Option<&str>,
    max_results: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
) -> Result<crate::SearchOutcome, ToolboxError> {
    if target.is_file() {
        let re = Regex::new(pattern)
            .map_err(|err| ToolboxError::Search(SearchError::InvalidRegex(err.to_string())))?;
        let content = read_text_file(root, &path_to_display(root, target), max_file_bytes)?;
        let rel_path = path_to_display(root, target);
        let mut matches = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            if re.is_match(line) {
                matches.push(SearchMatch {
                    path: rel_path.clone(),
                    line_num: idx + 1,
                    line: line.trim().to_string(),
                });
                if matches.len() >= max_results {
                    return Ok(crate::SearchOutcome {
                        matches,
                        truncated: true,
                    });
                }
            }
        }
        return Ok(crate::SearchOutcome {
            matches,
            truncated: false,
        });
    }

    search_file_contents(
        target,
        pattern,
        glob,
        max_results,
        max_file_bytes,
        max_total_bytes,
    )
    .map_err(Into::into)
}

fn resolve_copy_move_destination(
    root: &Path,
    source: &Path,
    destination: &str,
) -> Result<PathBuf, ToolboxError> {
    let candidate = resolve_new_path(root, destination, false)?;
    if candidate.exists() && candidate.is_dir() {
        let file_name = source.file_name().ok_or_else(|| {
            ToolboxError::InvalidArgument(format!(
                "cannot derive destination basename from {}",
                source.display()
            ))
        })?;
        let nested = candidate.join(file_name);
        ensure_existing_ancestor_within_root(root, &nested)?;
        return Ok(nested);
    }
    Ok(candidate)
}

fn copy_path(
    root: &Path,
    source: &Path,
    destination: &Path,
    recursive: bool,
    overwrite: bool,
    max_total_bytes: u64,
    max_entries: usize,
) -> Result<(), ToolboxError> {
    if source.is_file() {
        copy_file(source, destination, overwrite)?;
        return Ok(());
    }

    if !source.is_dir() {
        return Err(ToolboxError::InvalidArgument(format!(
            "unsupported source type: {}",
            source.display()
        )));
    }
    if !recursive {
        return Err(ToolboxError::RecursiveRequired(format!(
            "{} is a directory",
            path_to_display(root, source)
        )));
    }
    if destination.exists() {
        return Err(ToolboxError::AlreadyExists(path_to_display(
            root,
            destination,
        )));
    }

    let source_canonical = source.canonicalize().map_err(io_error)?;
    if destination.starts_with(&source_canonical) {
        return Err(ToolboxError::InvalidArgument(
            "refusing to copy a directory into itself".to_string(),
        ));
    }

    let mut total_bytes = 0u64;
    let mut entries_seen = 0usize;
    for entry in walkdir::WalkDir::new(source).into_iter().flatten() {
        let rel = entry.path().strip_prefix(source).map_err(|_| {
            ToolboxError::InvalidArgument("failed to compute relative path".to_string())
        })?;
        let target = destination.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).map_err(io_error)?;
        } else if entry.file_type().is_file() {
            entries_seen += 1;
            if entries_seen > max_entries {
                return Err(ToolboxError::TooLarge(format!(
                    "copy exceeded {max_entries} file entries"
                )));
            }
            let meta = entry
                .metadata()
                .map_err(|err| ToolboxError::Io(err.to_string()))?;
            total_bytes = total_bytes.saturating_add(meta.len());
            if total_bytes > max_total_bytes {
                return Err(ToolboxError::TooLarge(format!(
                    "copy exceeded {max_total_bytes} bytes"
                )));
            }
            copy_file(entry.path(), &target, overwrite)?;
        }
    }

    Ok(())
}

fn copy_file(source: &Path, destination: &Path, overwrite: bool) -> Result<(), ToolboxError> {
    let parent = destination
        .parent()
        .ok_or_else(|| ToolboxError::InvalidArgument(destination.display().to_string()))?;
    std::fs::create_dir_all(parent).map_err(io_error)?;
    if destination.exists() {
        if destination.is_dir() {
            return Err(ToolboxError::AlreadyExists(
                destination.display().to_string(),
            ));
        }
        if !overwrite {
            return Err(ToolboxError::AlreadyExists(
                destination.display().to_string(),
            ));
        }
    }
    std::fs::copy(source, destination).map_err(io_error)?;
    Ok(())
}

fn move_path(
    root: &Path,
    source: &Path,
    destination: &Path,
    overwrite: bool,
) -> Result<(), ToolboxError> {
    if destination.exists() {
        if !overwrite {
            return Err(ToolboxError::AlreadyExists(path_to_display(
                root,
                destination,
            )));
        }
        if destination.is_file() {
            std::fs::remove_file(destination).map_err(io_error)?;
        } else {
            return Err(ToolboxError::AlreadyExists(path_to_display(
                root,
                destination,
            )));
        }
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    std::fs::rename(source, destination).map_err(io_error)
}

fn remove_path(target: &Path, recursive: bool, max_entries: usize) -> Result<(), ToolboxError> {
    if target.is_file() {
        return std::fs::remove_file(target).map_err(io_error);
    }
    if !target.is_dir() {
        return Err(ToolboxError::NotFound(target.display().to_string()));
    }
    if !recursive {
        return std::fs::remove_dir(target).map_err(io_error);
    }

    let mut entries_seen = 0usize;
    for entry in walkdir::WalkDir::new(target)
        .min_depth(1)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() || entry.file_type().is_dir() {
            entries_seen += 1;
            if entries_seen > max_entries {
                return Err(ToolboxError::TooLarge(format!(
                    "remove exceeded {max_entries} entries"
                )));
            }
        }
    }
    std::fs::remove_dir_all(target).map_err(io_error)
}

fn compile_glob(glob: &str) -> Result<Regex, ToolboxError> {
    let re_str = glob
        .replace('.', "\\.")
        .replace("**", "<<GLOBSTAR>>")
        .replace('*', "[^/]*")
        .replace("<<GLOBSTAR>>", ".*");
    Regex::new(&format!("{re_str}$")).map_err(|err| ToolboxError::InvalidGlob(err.to_string()))
}

fn path_to_display(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            parts.push(part.to_string_lossy().to_string());
        }
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

fn io_error(err: std::io::Error) -> ToolboxError {
    ToolboxError::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "brokk-acp-sandbox-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_only_commands_return_expected_shapes() {
        let root = temp_workspace("readonly");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {}\nprintln!(\"hello\");\nthird line\n",
        )
        .unwrap();

        let head = run_workspace_command(
            &root,
            &WorkspaceCommand::Head {
                path: "src/main.rs".to_string(),
                lines: 2,
                max_bytes: None,
            },
        )
        .unwrap();
        assert_eq!(
            head,
            WorkspaceCommandOutput::Head {
                content: "fn main() {}\nprintln!(\"hello\");\n".to_string(),
            }
        );

        let tail = run_workspace_command(
            &root,
            &WorkspaceCommand::Tail {
                path: "src/main.rs".to_string(),
                lines: 1,
                max_bytes: None,
            },
        )
        .unwrap();
        assert_eq!(
            tail,
            WorkspaceCommandOutput::Tail {
                content: "third line\n".to_string(),
            }
        );

        let wc = run_workspace_command(
            &root,
            &WorkspaceCommand::Wc {
                path: "src/main.rs".to_string(),
                max_bytes: None,
            },
        )
        .unwrap();
        match wc {
            WorkspaceCommandOutput::Wc { counts } => {
                assert_eq!(counts.lines, 3);
                assert!(counts.words >= 4);
            }
            other => panic!("unexpected output: {other:?}"),
        }

        let find = run_workspace_command(
            &root,
            &WorkspaceCommand::Find {
                path: Some("src".to_string()),
                glob: Some("**/*.rs".to_string()),
                max_results: None,
            },
        )
        .unwrap();
        assert_eq!(
            find,
            WorkspaceCommandOutput::Find {
                paths: vec!["src/main.rs".to_string()],
                truncated: false,
            }
        );

        let grep = run_workspace_command(
            &root,
            &WorkspaceCommand::Grep {
                path: Some("src/main.rs".to_string()),
                pattern: "println".to_string(),
                glob: None,
                max_results: 10,
                max_file_bytes: 1024,
                max_total_bytes: 4096,
            },
        )
        .unwrap();
        match grep {
            WorkspaceCommandOutput::Grep { matches, truncated } => {
                assert!(!truncated);
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path, "src/main.rs");
                assert_eq!(matches[0].line_num, 2);
            }
            other => panic!("unexpected output: {other:?}"),
        }

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn mutation_commands_stay_in_workspace() {
        let root = temp_workspace("mutations");
        std::fs::write(root.join("note.txt"), "hello world\n").unwrap();

        run_workspace_command(
            &root,
            &WorkspaceCommand::Mkdir {
                path: "nested".to_string(),
                parents: false,
            },
        )
        .unwrap();
        assert!(root.join("nested").is_dir());

        run_workspace_command(
            &root,
            &WorkspaceCommand::Cp {
                source: "note.txt".to_string(),
                destination: "nested/copy.txt".to_string(),
                recursive: false,
                overwrite: false,
                max_total_bytes: None,
                max_entries: None,
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("nested/copy.txt")).unwrap(),
            "hello world\n"
        );

        run_workspace_command(
            &root,
            &WorkspaceCommand::Mv {
                source: "nested/copy.txt".to_string(),
                destination: "nested/moved.txt".to_string(),
                overwrite: false,
            },
        )
        .unwrap();
        assert!(root.join("nested/moved.txt").is_file());
        assert!(!root.join("nested/copy.txt").exists());

        run_workspace_command(
            &root,
            &WorkspaceCommand::Rm {
                path: "nested/moved.txt".to_string(),
                recursive: false,
                max_entries: None,
            },
        )
        .unwrap();
        assert!(!root.join("nested/moved.txt").exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn escaping_paths_are_rejected() {
        let root = temp_workspace("escape");
        let err = run_workspace_command(
            &root,
            &WorkspaceCommand::Cat {
                path: "../outside.txt".to_string(),
                max_bytes: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, ToolboxError::PathEscapesWorkspace(_)));

        std::fs::remove_dir_all(root).ok();
    }
}
