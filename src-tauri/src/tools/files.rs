//! The file tools (spec §9.4): `files.search`, `files.read_text`, `files.move`,
//! `files.rename`. Every path goes through `PathScope`, and again right before acting.
//! There is no delete, write or overwrite.

use super::{Prepared, Tool, ToolContext, ToolError, ToolOutput, clean_for_summary, parse_args};
use crate::config::display_path;
use crate::ipc::types::ToolRisk;
use crate::permissions::paths::{PathError, PathScope};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Largest file `files.read_text` will read (spec §9.4).
pub const MAX_READ_BYTES: u64 = 1024 * 1024;
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "log", "toml", "yaml", "yml", "ini", "xml",
    "html", "htm", "rs", "py", "js", "ts", "tsx", "jsx", "java", "c", "h", "cpp", "go", "sh",
    "tex", "rst",
];
const DEFAULT_RESULTS: usize = 20;
const MAX_RESULTS: usize = 50;
const MAX_SEARCH_DEPTH: usize = 8;
const MAX_SEARCH_VISITED: usize = 20_000;
const MAX_QUERY_CHARS: usize = 100;

fn scope_error(e: PathError) -> ToolError {
    match e {
        PathError::OutsideRoots | PathError::DevicePath | PathError::NotAbsolute => {
            ToolError::NotAllowed("That path is not inside an allowed folder.".into())
        }
        PathError::Missing => ToolError::Invalid("That path does not exist.".into()),
        PathError::Invalid => ToolError::Invalid("That path is not valid.".into()),
    }
}

fn name_of(p: &Path) -> String {
    clean_for_summary(
        &p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

fn no_folders(ctx: &ToolContext) -> Result<(), ToolError> {
    if ctx.scope.roots().is_empty() {
        return Err(ToolError::NotAllowed(
            "No folders are allowed yet. Ask the user to add one in Settings.".into(),
        ));
    }
    Ok(())
}

// ------------------------------------------------------------------ files.search

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    /// Empty (or left out) lists the top level of the folder instead of searching.
    #[serde(default)]
    query: String,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    max_results: Option<u8>,
}

fn search_inputs(
    a: &SearchArgs,
    ctx: &ToolContext,
) -> Result<(String, Vec<PathBuf>, usize), ToolError> {
    no_folders(ctx)?;
    let q = a.query.trim();
    if q.chars().count() > MAX_QUERY_CHARS || q.chars().any(char::is_control) {
        return Err(ToolError::Invalid(
            "The search text must be at most 100 printable characters.".into(),
        ));
    }
    let roots = match &a.folder {
        None => ctx.scope.roots(),
        Some(f) => {
            let p = ctx.scope.check_existing(f).map_err(scope_error)?;
            if !p.is_dir() {
                return Err(ToolError::Invalid("That path is not a folder.".into()));
            }
            vec![p]
        }
    };
    let max = usize::from(a.max_results.unwrap_or(DEFAULT_RESULTS as u8)).clamp(1, MAX_RESULTS);
    Ok((q.to_lowercase(), roots, max))
}

/// Name search below the roots. Symlinks are skipped (never followed) and the walk is
/// bounded in depth and in entries visited. An empty `needle` lists only the top level of
/// each root, without going into sub-folders.
fn search(
    roots: Vec<PathBuf>,
    needle: &str,
    max: usize,
    cancel: &CancellationToken,
) -> Result<(Vec<(bool, PathBuf)>, bool), ToolError> {
    let mut found = Vec::new();
    let list_only = needle.is_empty();
    let (mut visited, mut truncated) = (0usize, false);
    let mut stack: Vec<(PathBuf, usize)> = roots.into_iter().map(|r| (r, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            visited += 1;
            if visited % 256 == 0 && cancel.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            if visited > MAX_SEARCH_VISITED {
                return Ok((found, true));
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let is_dir = ft.is_dir();
            if e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains(needle)
            {
                if found.len() >= max {
                    truncated = true;
                    return Ok((found, truncated));
                }
                found.push((is_dir, e.path()));
            }
            if is_dir && !list_only && depth < MAX_SEARCH_DEPTH {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    Ok((found, truncated))
}

pub struct FilesSearch;

#[async_trait]
impl Tool for FilesSearch {
    fn name(&self) -> &'static str {
        "files.search"
    }
    fn description(&self) -> &'static str {
        "Find files and folders by part of their name inside the folders the user allowed, or list what is in a folder by leaving the query empty. Names only, never contents."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Part of the name to look for (up to 100 characters). Leave empty to list the top level of the folder." },
                "folder": { "type": "string", "description": "Optional absolute path of an allowed folder to search; default is all allowed folders." },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 50 }
            },
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(15)
    }
    fn prepare(&self, args: &Value, ctx: &ToolContext) -> Result<Prepared, ToolError> {
        let a: SearchArgs = parse_args(args)?;
        let (_, roots, _) = search_inputs(&a, ctx)?;
        Ok(Prepared {
            arg_risk: None,
            targets: roots.iter().map(|r| display_path(r)).collect(),
            summary: if a.query.trim().is_empty() {
                "List what is in the allowed folder".to_owned()
            } else {
                format!(
                    "Search for files named \"{}\"",
                    clean_for_summary(a.query.trim())
                )
            },
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: SearchArgs = parse_args(args)?;
        let (needle, roots, max) = search_inputs(&a, ctx)?;
        let (found, truncated) =
            tokio::task::spawn_blocking(move || search(roots, &needle, max, &cancel))
                .await
                .map_err(|_| ToolError::Failed("The search failed.".into()))??;
        let mut text = if found.is_empty() {
            "No matching files or folders.".to_owned()
        } else {
            found
                .iter()
                .map(|(dir, p)| {
                    format!(
                        "{}: {}",
                        if *dir { "folder" } else { "file" },
                        display_path(p)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        if truncated {
            text.push_str("\n(more results exist; narrow the search)");
        }
        // File names come from disk and can say anything, so they count as outside content.
        Ok(ToolOutput {
            text,
            external_content: true,
        })
    }
}

// ------------------------------------------------------------------ files.read_text

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
}

fn resolve_readable(a: &ReadArgs, ctx: &ToolContext) -> Result<(PathBuf, u64), ToolError> {
    no_folders(ctx)?;
    let p = ctx.scope.check_existing(&a.path).map_err(scope_error)?;
    let meta = std::fs::metadata(&p)
        .map_err(|_| ToolError::Invalid("That file cannot be read.".into()))?;
    if !meta.is_file() {
        return Err(ToolError::Invalid("That path is not a file.".into()));
    }
    let ok_ext = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| TEXT_EXTENSIONS.contains(&e.to_lowercase().as_str()));
    if !ok_ext {
        return Err(ToolError::Invalid(
            "Only plain text file types can be read.".into(),
        ));
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(ToolError::Invalid("That file is larger than 1 MB.".into()));
    }
    Ok((p, meta.len()))
}

pub struct FilesReadText;

#[async_trait]
impl Tool for FilesReadText {
    fn name(&self) -> &'static str {
        "files.read_text"
    }
    fn description(&self) -> &'static str {
        "Read a plain text file (up to 1 MB) inside an allowed folder. The user must approve, and its contents go to the AI provider."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Absolute path of the file." } },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Medium
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn prepare(&self, args: &Value, ctx: &ToolContext) -> Result<Prepared, ToolError> {
        let a: ReadArgs = parse_args(args)?;
        let (p, len) = resolve_readable(&a, ctx)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![display_path(&p)],
            summary: format!(
                "Read the file \"{}\" ({} KB). Its contents will be sent to the AI provider.",
                name_of(&p),
                len.div_ceil(1024)
            ),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: ReadArgs = parse_args(args)?;
        // Checked again here: the file may have been swapped since approval.
        let (p, _) = resolve_readable(&a, ctx)?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let bytes = tokio::task::spawn_blocking(move || {
            let mut buf = Vec::new();
            std::fs::File::open(&p)?
                .take(MAX_READ_BYTES + 1)
                .read_to_end(&mut buf)?;
            Ok::<_, std::io::Error>(buf)
        })
        .await
        .map_err(|_| ToolError::Failed("Reading failed.".into()))?
        .map_err(|_| ToolError::Failed("The file could not be read.".into()))?;
        if bytes.len() as u64 > MAX_READ_BYTES {
            return Err(ToolError::Invalid("That file is larger than 1 MB.".into()));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| ToolError::Invalid("That file is not valid UTF-8 text.".into()))?;
        Ok(ToolOutput {
            text,
            external_content: true,
        })
    }
}

// ------------------------------------------------------------------ files.move / files.rename

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveArgs {
    from: String,
    to: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameArgs {
    path: String,
    new_name: String,
}

struct Plan {
    from: PathBuf,
    to: PathBuf,
    is_dir: bool,
}

fn plan_move(scope: &PathScope, from: &str, to: &str) -> Result<Plan, ToolError> {
    let from = scope.check_existing(from).map_err(scope_error)?;
    if scope.is_root(&from) {
        return Err(ToolError::NotAllowed(
            "An allowed folder itself cannot be moved or renamed.".into(),
        ));
    }
    let to = scope.check_new(to).map_err(scope_error)?;
    check_destination(&from, &to)?;
    Ok(Plan {
        is_dir: from.is_dir(),
        from,
        to,
    })
}

fn check_destination(from: &Path, to: &Path) -> Result<(), ToolError> {
    if to == from {
        return Err(ToolError::Invalid(
            "The source and destination are the same.".into(),
        ));
    }
    if to.symlink_metadata().is_ok() {
        return Err(ToolError::Invalid(
            "The destination already exists; nothing is overwritten.".into(),
        ));
    }
    if from.is_dir() && to.starts_with(from) {
        return Err(ToolError::Invalid(
            "A folder cannot be moved into itself.".into(),
        ));
    }
    Ok(())
}

fn plan_rename(scope: &PathScope, path: &str, new_name: &str) -> Result<Plan, ToolError> {
    let from = scope.check_existing(path).map_err(scope_error)?;
    if scope.is_root(&from) {
        return Err(ToolError::NotAllowed(
            "An allowed folder itself cannot be moved or renamed.".into(),
        ));
    }
    let new_name = new_name.trim();
    if !PathScope::leaf_ok(new_name) {
        return Err(ToolError::Invalid("That is not a valid file name.".into()));
    }
    let parent = from
        .parent()
        .ok_or_else(|| ToolError::Invalid("That path is not valid.".into()))?;
    let to = scope
        .check_new(&display_path(&parent.join(new_name)))
        .map_err(scope_error)?;
    check_destination(&from, &to)?;
    Ok(Plan {
        is_dir: from.is_dir(),
        from,
        to,
    })
}

fn kind(is_dir: bool) -> &'static str {
    if is_dir { "folder" } else { "file" }
}

/// Moving or renaming a whole folder affects everything inside it, so it is high risk.
fn plan_risk(p: &Plan) -> Option<ToolRisk> {
    p.is_dir.then_some(ToolRisk::High)
}

async fn do_rename(plan: Plan, cancel: &CancellationToken) -> Result<ToolOutput, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    let Plan { from, to, .. } = plan;
    // The OS refuses an existing destination atomically (no check-then-replace window).
    tokio::task::spawn_blocking(move || crate::platform::fs::rename_no_replace(&from, &to))
        .await
        .map_err(|_| ToolError::Failed("The move failed.".into()))?
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::CrossesDevices => ToolError::Failed("The destination is on a different drive; moving across drives is not supported yet.".into()),
            std::io::ErrorKind::AlreadyExists => ToolError::Failed("Something already exists at the destination; nothing was changed.".into()),
            _ => ToolError::Failed("The move failed.".into()),
        })?;
    Ok(ToolOutput {
        text: "Done.".into(),
        external_content: false,
    })
}

pub struct FilesMove;

#[async_trait]
impl Tool for FilesMove {
    fn name(&self) -> &'static str {
        "files.move"
    }
    fn description(&self) -> &'static str {
        "Move a file or folder to a new place inside the allowed folders. The user must approve. Nothing is ever overwritten or deleted."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "description": "Absolute path of the file or folder to move." },
                "to": { "type": "string", "description": "Absolute path it should have afterwards (must not exist)." }
            },
            "required": ["from", "to"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Medium
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn prepare(&self, args: &Value, ctx: &ToolContext) -> Result<Prepared, ToolError> {
        let a: MoveArgs = parse_args(args)?;
        no_folders(ctx)?;
        let p = plan_move(&ctx.scope, &a.from, &a.to)?;
        Ok(Prepared {
            arg_risk: plan_risk(&p),
            targets: vec![display_path(&p.from), display_path(&p.to)],
            summary: format!(
                "Move the {} \"{}\" to \"{}\"",
                kind(p.is_dir),
                name_of(&p.from),
                name_of(&p.to)
            ),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: MoveArgs = parse_args(args)?;
        do_rename(plan_move(&ctx.scope, &a.from, &a.to)?, &cancel).await
    }
}

pub struct FilesRename;

#[async_trait]
impl Tool for FilesRename {
    fn name(&self) -> &'static str {
        "files.rename"
    }
    fn description(&self) -> &'static str {
        "Rename a file or folder inside the allowed folders (same location, new name). The user must approve. Nothing is overwritten."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file or folder." },
                "new_name": { "type": "string", "description": "The new name only, not a path." }
            },
            "required": ["path", "new_name"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Medium
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn prepare(&self, args: &Value, ctx: &ToolContext) -> Result<Prepared, ToolError> {
        let a: RenameArgs = parse_args(args)?;
        no_folders(ctx)?;
        let p = plan_rename(&ctx.scope, &a.path, &a.new_name)?;
        Ok(Prepared {
            arg_risk: plan_risk(&p),
            targets: vec![display_path(&p.from), display_path(&p.to)],
            summary: format!(
                "Rename the {} \"{}\" to \"{}\"",
                kind(p.is_dir),
                name_of(&p.from),
                name_of(&p.to)
            ),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: RenameArgs = parse_args(args)?;
        do_rename(plan_rename(&ctx.scope, &a.path, &a.new_name)?, &cancel).await
    }
}

#[cfg(test)]
mod race_tests {
    use super::*;

    #[tokio::test]
    async fn a_destination_created_after_the_check_is_never_overwritten() {
        let d = std::env::temp_dir().join(format!("jarvis-race-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "A").unwrap();
        // The plan was made while `b.txt` did not exist; now it appears.
        let plan = Plan {
            from: d.join("a.txt"),
            to: d.join("b.txt"),
            is_dir: false,
        };
        std::fs::write(d.join("b.txt"), "B").unwrap();
        let r = do_rename(plan, &CancellationToken::new()).await;
        assert!(matches!(r, Err(ToolError::Failed(m)) if m.contains("already exists")));
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "B");
    }
}
