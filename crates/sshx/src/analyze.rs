//! Workspace source-code analysis database.
//!
//! The [`AnalysisDb`] is a JSON file (`.sshx-analysis.json`) that lives in the
//! workspace root. It holds per-file structural metadata (imports, exports,
//! line counts, timestamps) and an optional AI-generated description field that
//! starts empty and can be filled in externally (e.g. via the UI).
//!
//! # Workflow
//!
//! ```text
//! sshx analyze [--workspace PATH]   # builds / refreshes the file
//! sshx                               # reads the file and streams it to the browser
//! ```

#![allow(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default filename written into the workspace root.
pub const ANALYSIS_FILENAME: &str = ".sshx-analysis.json";

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".svelte-kit",
];

const INCLUDE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "svelte", "json", "py", "go",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The on-disk analysis database, serialised as JSON.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisDb {
    /// Schema version for forward-compatibility.
    pub version: u32,
    /// Absolute path of the workspace root that was analysed.
    pub workspace_root: String,
    /// ISO 8601 timestamp of when `build()` was last called.
    pub analyzed_at: String,
    /// Per-file entries, keyed by their relative path from the workspace root.
    pub files: HashMap<String, AnalysisEntry>,
}

/// Per-file entry in the analysis database.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisEntry {
    /// Relative path from the workspace root (forward-slash separated).
    pub path: String,
    /// File kind: `"component"` | `"hook"` | `"utility"` | `"config"` |
    /// `"type"` | `"other"`.
    pub kind: String,
    /// Local (relative) import paths made by this file.
    pub local_imports: Vec<String>,
    /// Library / package names imported by this file.
    pub libraries: Vec<String>,
    /// Exported symbol names.
    pub exports: Vec<String>,
    /// Relative paths of files that import this file (computed in second pass).
    pub imported_by: Vec<String>,
    /// Total number of source lines.
    pub line_count: u32,
    /// ISO 8601 timestamp of the file's last on-disk modification.
    pub last_modified: String,
    /// ISO 8601 timestamp of the last time this entry was (re-)analysed.
    pub last_analyzed: String,
    /// Optional AI-generated one-sentence description (≤120 chars).
    /// Empty until filled in externally.
    pub description: String,
    /// First 6144 characters of the file, for preview in the UI.
    #[serde(default)]
    pub content: String,

    /// URL or path of an illustration image attached to this file card.
    /// Empty until set by the user.
    #[serde(default)]
    pub image_path: String,

    /// Last known widget width in pixels (0 = use default).
    #[serde(default)]
    pub widget_w: u32,

    /// Last known widget height in pixels (0 = use default).
    #[serde(default)]
    pub widget_h: u32,
}

// ---------------------------------------------------------------------------
// Graph types
// ---------------------------------------------------------------------------

/// A node in the component graph representing a single source file.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Relative path from the workspace root (same as the node key).
    pub path: String,
    /// File kind: "component"|"hook"|"utility"|"config"|"type"|"other".
    pub kind: String,
    /// Filename portion of the path, for display labels.
    pub label: String,
    /// AI-generated or empty description.
    pub description: String,
    /// Total source lines.
    pub line_count: u32,
}

/// A directed edge in the component graph.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    /// Source node path (the file that imports).
    pub from: String,
    /// Target node path (the file being imported).
    pub to: String,
}

/// Graph of components and their relationships.
///
/// Two edge sets are kept:
/// - `code_edges`: static import/use relationships derived from the analysis.
/// - `display_edges`: visual/render-tree relationships (populated later by the UI).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ComponentGraph {
    /// Nodes keyed by relative file path for O(1) adjacency look-up.
    pub nodes: HashMap<String, GraphNode>,
    /// Directed code-dependency edges (importer → importee).
    pub code_edges: Vec<GraphEdge>,
    /// Directed display/render-tree edges (to be populated by the UI).
    pub display_edges: Vec<GraphEdge>,
}

// ---------------------------------------------------------------------------
// AnalysisDb impl
// ---------------------------------------------------------------------------

impl AnalysisDb {
    /// Walk `root`, parse every included file, and return a fresh database.
    ///
    /// Descriptions are left empty; use [`AnalysisDb::merge_with_existing`]
    /// to preserve any previously written descriptions.
    pub fn build(root: &Path) -> Result<Self> {
        let now = format_iso8601(current_unix_secs());
        let paths = collect_files(root);
        let mut entries: Vec<AnalysisEntry> = Vec::with_capacity(paths.len());

        for path in &paths {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let mut e = parse_file(root, path, &content);
                    e.last_analyzed = now.clone();
                    entries.push(e);
                }
                Err(err) => {
                    tracing::debug!("skipping {}: {err}", path.display());
                }
            }
        }

        compute_imported_by(&mut entries);

        let files: HashMap<String, AnalysisEntry> = entries
            .into_iter()
            .map(|e| (e.path.clone(), e))
            .collect();

        Ok(AnalysisDb {
            version: 1,
            workspace_root: root.to_string_lossy().into_owned(),
            analyzed_at: now,
            files,
        })
    }

    /// Copy descriptions (and `lastAnalyzed`) from `existing` for any file
    /// whose `lastModified` timestamp has not changed since that analysis.
    pub fn merge_with_existing(mut self, existing: &AnalysisDb) -> Self {
        for (path, entry) in self.files.iter_mut() {
            if let Some(old) = existing.files.get(path) {
                if !old.description.is_empty()
                    && entry.last_modified == old.last_modified
                {
                    entry.description = old.description.clone();
                    entry.last_analyzed = old.last_analyzed.clone();
                }
                // Always preserve user-set metadata regardless of file modification.
                if !old.image_path.is_empty() {
                    entry.image_path = old.image_path.clone();
                }
                if old.widget_w > 0 {
                    entry.widget_w = old.widget_w;
                }
                if old.widget_h > 0 {
                    entry.widget_h = old.widget_h;
                }
            }
        }
        self
    }

    /// Save the database as pretty-printed JSON to `path`.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load an existing database from `path`.
    ///
    /// Returns `None` if the file does not exist.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Return the number of files in the database.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Return the number of files that have a non-empty description.
    pub fn described_count(&self) -> usize {
        self.files.values().filter(|e| !e.description.is_empty()).count()
    }

    /// Derive a [`ComponentGraph`] from this database.
    ///
    /// Nodes are one-per-file; code edges come from the already-resolved
    /// `imported_by` field so no path resolution is needed here.
    pub fn to_graph(&self) -> ComponentGraph {
        let nodes: HashMap<String, GraphNode> = self
            .files
            .values()
            .map(|e| {
                let label = Path::new(&e.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&e.path)
                    .to_string();
                (
                    e.path.clone(),
                    GraphNode {
                        path: e.path.clone(),
                        kind: e.kind.clone(),
                        label,
                        description: e.description.clone(),
                        line_count: e.line_count,
                    },
                )
            })
            .collect();

        // Build code edges from `imported_by` (already resolved in second pass).
        let mut code_edges: Vec<GraphEdge> = Vec::new();
        for entry in self.files.values() {
            for importer in &entry.imported_by {
                code_edges.push(GraphEdge {
                    from: importer.clone(),
                    to: entry.path.clone(),
                });
            }
        }
        // Stable order for deterministic output.
        code_edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));

        ComponentGraph {
            nodes,
            code_edges,
            display_edges: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// File collection
// ---------------------------------------------------------------------------

fn collect_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    return !SKIP_DIRS.contains(&name);
                }
            }
            true
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|ext| INCLUDE_EXTS.contains(&ext))
                    .unwrap_or(false)
        })
        .map(|e| e.path().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Per-file parsing
// ---------------------------------------------------------------------------

// Lazily compiled regex helper.
macro_rules! re {
    ($pat:literal) => {{
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| Regex::new($pat).unwrap())
    }};
}

fn detect_kind(path: &Path, content: &str) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "json" => return "config".into(),
        "ts" if path
            .to_str()
            .map(|s| s.ends_with(".d.ts"))
            .unwrap_or(false) =>
        {
            return "type".into()
        }
        _ => {}
    }
    if matches!(ext, "tsx" | "svelte" | "jsx") {
        return "component".into();
    }
    if re!(r"\buse[A-Z]").is_match(content) {
        return "hook".into();
    }
    if content.contains("export type ") || content.contains("export interface ") {
        return "type".into();
    }
    "utility".into()
}

fn parse_file(root: &Path, path: &Path, content: &str) -> AnalysisEntry {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    let mut local_imports: Vec<String> = Vec::new();
    let mut libraries: Vec<String> = Vec::new();
    let mut exports: Vec<String> = Vec::new();

    // Collect import sources.
    let mut all_imports: Vec<String> = Vec::new();
    for cap in re!(r#"(?:from|import)\s+['"]([^'"]+)['"]"#).captures_iter(content) {
        all_imports.push(cap[1].to_string());
    }
    for cap in re!(r#"require\(\s*['"]([^'"]+)['"]\s*\)"#).captures_iter(content) {
        all_imports.push(cap[1].to_string());
    }
    for cap in re!(r#"import\(\s*['"]([^'"]+)['"]\s*\)"#).captures_iter(content) {
        all_imports.push(cap[1].to_string());
    }

    for src in all_imports {
        if src.starts_with('.') {
            local_imports.push(src);
        } else {
            let pkg = if src.starts_with('@') {
                src.splitn(3, '/').take(2).collect::<Vec<_>>().join("/")
            } else {
                src.split('/').next().unwrap_or(&src).to_string()
            };
            if !libraries.contains(&pkg) {
                libraries.push(pkg);
            }
        }
    }
    local_imports.dedup();

    // Collect exports.
    for cap in
        re!(r#"export\s+(?:const|let|var|function|class|type|interface|enum)\s+(\w+)"#)
            .captures_iter(content)
    {
        exports.push(cap[1].to_string());
    }
    for cap in re!(r#"export\s*\{([^}]+)\}"#).captures_iter(content) {
        for name in cap[1].split(',') {
            let name = name.trim().split(' ').next().unwrap_or("").to_string();
            if !name.is_empty() {
                exports.push(name);
            }
        }
    }
    if re!(r#"export\s+default\s"#).is_match(content) {
        exports.push("default".into());
    }
    exports.dedup();

    let line_count = content.lines().count() as u32;

    let last_modified = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| format_iso8601(d.as_secs()))
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let preview: String = content.chars().take(6144).collect();

    AnalysisEntry {
        path: rel,
        kind: detect_kind(path, content),
        local_imports,
        libraries,
        exports,
        imported_by: Vec::new(), // filled in second pass
        line_count,
        last_modified,
        last_analyzed: String::new(), // set by caller
        description: String::new(),
        content: preview,
        image_path: String::new(),
        widget_w: 0,
        widget_h: 0,
    }
}

fn compute_imported_by(entries: &mut Vec<AnalysisEntry>) {
    let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let mut imported_by: HashMap<String, Vec<String>> = HashMap::new();

    for entry in entries.iter() {
        for imp in &entry.local_imports {
            let dir = Path::new(&entry.path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(target) = resolve_import(&dir, imp, &paths) {
                imported_by
                    .entry(target)
                    .or_default()
                    .push(entry.path.clone());
            }
        }
    }

    for entry in entries.iter_mut() {
        if let Some(importers) = imported_by.get(&entry.path) {
            entry.imported_by = importers.clone();
        }
    }
}

fn resolve_import(from_dir: &str, import: &str, all_paths: &[String]) -> Option<String> {
    let base = if from_dir.is_empty() {
        import.to_string()
    } else {
        format!("{from_dir}/{import}")
    };
    let base = normalize_path(&base);

    for path in all_paths {
        let p = path.trim_start_matches("./");
        if p == base || p.starts_with(&format!("{base}/index.")) {
            return Some(path.clone());
        }
        let stem = path.rfind('.').map(|i| &path[..i]).unwrap_or(path);
        if stem == base {
            return Some(path.clone());
        }
    }
    None
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
// Timestamp helpers
// ---------------------------------------------------------------------------

pub(crate) fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn format_iso8601(secs: u64) -> String {
    let mut rem = secs;
    let s = rem % 60;
    rem /= 60;
    let mi = rem % 60;
    rem /= 60;
    let h = rem % 24;
    rem /= 24;
    let (y, mo, d) = days_to_ymd(rem as u32);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn days_to_ymd(mut days: u32) -> (u32, u32, u32) {
    let mut year = 1970u32;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days: &[u32] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u32;
    for &md in month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}
