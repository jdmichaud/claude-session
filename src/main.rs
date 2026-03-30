use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use colored::Colorize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "cs", about = "Manage Claude Code sessions")]
struct Cli {
    /// Project directory (defaults to current directory)
    #[arg(short, long)]
    dir: Option<PathBuf>,

    /// Folder to scan for sessions (shows tree view when given without a subcommand)
    folder: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List sessions (default when no subcommand given)
    List,
    /// Delete a session by ID (uuid or slug prefix)
    Delete {
        /// Session identifier (uuid prefix or slug prefix)
        session: String,
    },
    /// Print the conversation of a session to stdout
    Show {
        /// Session identifier (uuid prefix or slug prefix)
        session: String,
    },
}

struct SessionInfo {
    uuid: String,
    title: Option<String>,
    path: PathBuf,
    timestamp: Option<DateTime<Utc>>,
    first_message: Option<String>,
    message_count: usize,
}

struct DirNode {
    name: String,
    sessions: Vec<SessionInfo>,
    children: BTreeMap<String, DirNode>,
}

fn claude_projects_dir() -> PathBuf {
    let home = dirs::home_dir().expect("cannot determine home directory");
    home.join(".claude").join("projects")
}

fn project_key(dir: &Path) -> String {
    let abs = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    abs.to_string_lossy().replace('/', "-")
}

fn session_dir(dir: &Path) -> PathBuf {
    claude_projects_dir().join(project_key(dir))
}

fn list_session_files(dir: &Path) -> Vec<PathBuf> {
    let sdir = session_dir(dir);
    if !sdir.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&sdir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|ext| ext == "jsonl")
                && p.file_stem()
                    .is_some_and(|s| s.len() == 36 && s.to_string_lossy().contains('-'))
        })
        .collect();
    files.sort();
    files
}

fn parse_session(path: &Path) -> SessionInfo {
    let uuid = path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let file = fs::File::open(path).expect("cannot open session file");
    let reader = BufReader::new(file);

    let mut title: Option<String> = None;
    let mut has_custom_title = false;
    let mut timestamp: Option<DateTime<Utc>> = None;
    let mut first_message: Option<String> = None;
    let mut message_count: usize = 0;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(val) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Prefer customTitle over slug
        if msg_type == "custom-title"
            && let Some(t) = val.get("customTitle").and_then(|v| v.as_str())
        {
            title = Some(t.to_string());
            has_custom_title = true;
        }
        if !has_custom_title
            && title.is_none()
            && let Some(s) = val.get("slug").and_then(|v| v.as_str())
        {
            title = Some(s.to_string());
        }

        match msg_type {
            "user" => {
                message_count += 1;
                if timestamp.is_none()
                    && let Some(ts) = val.get("timestamp").and_then(|v| v.as_str())
                {
                    timestamp = ts.parse().ok();
                }
                if first_message.is_none()
                    && let Some(content) = val
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                {
                    let truncated = if content.len() > 80 {
                        format!("{}...", &content[..77])
                    } else {
                        content.to_string()
                    };
                    first_message = Some(truncated.replace('\n', " "));
                }
            }
            "assistant" => {
                message_count += 1;
            }
            _ => {}
        }
    }

    SessionInfo {
        uuid,
        title,
        path: path.to_path_buf(),
        timestamp,
        first_message,
        message_count,
    }
}

fn resolve_session<'a>(sessions: &'a [SessionInfo], query: &str) -> Option<&'a SessionInfo> {
    let q = query.to_lowercase();
    // Try exact uuid match first
    if let Some(s) = sessions.iter().find(|s| s.uuid == q) {
        return Some(s);
    }
    // Try uuid prefix
    if let Some(s) = sessions.iter().find(|s| s.uuid.starts_with(&q)) {
        return Some(s);
    }
    // Try exact title match
    if let Some(s) = sessions
        .iter()
        .find(|s| s.title.as_deref().is_some_and(|t| t.to_lowercase() == q))
    {
        return Some(s);
    }
    // Try title prefix
    sessions
        .iter()
        .find(|s| s.title.as_deref().is_some_and(|t| t.to_lowercase().starts_with(&q)))
}

fn format_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "text" => {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                    "tool_use" => {
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let input = item.get("input").cloned().unwrap_or(Value::Null);
                        // Show a compact representation of tool calls
                        let input_summary = match &input {
                            Value::Object(map) => {
                                let keys: Vec<&String> = map.keys().collect();
                                if keys.len() <= 3 {
                                    // Show key=value for small inputs
                                    map.iter()
                                        .map(|(k, v)| {
                                            let val = match v {
                                                Value::String(s) => {
                                                    if s.len() > 60 {
                                                        format!("\"{}...\"", &s[..57])
                                                    } else {
                                                        format!("\"{s}\"")
                                                    }
                                                }
                                                _ => {
                                                    let s = v.to_string();
                                                    if s.len() > 60 {
                                                        format!("{}...", &s[..57])
                                                    } else {
                                                        s
                                                    }
                                                }
                                            };
                                            format!("{k}={val}")
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                } else {
                                    format!("{} params", keys.len())
                                }
                            }
                            _ => String::new(),
                        };
                        parts.push(format!("[tool: {name}({input_summary})]"));
                    }
                    "tool_result" => {
                        if let Some(text) = item.get("content").and_then(|v| v.as_str()) {
                            let truncated = if text.len() > 200 {
                                format!("{}...", &text[..197])
                            } else {
                                text.to_string()
                            };
                            parts.push(format!("[result: {truncated}]"));
                        }
                    }
                    "thinking" => {
                        // Skip thinking blocks
                    }
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn show_session(path: &Path) {
    let file = fs::File::open(path).expect("cannot open session file");
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(val) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let is_sidechain = val.get("isSidechain").and_then(|v| v.as_bool()).unwrap_or(false);

        if is_sidechain {
            continue;
        }

        let timestamp = val
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match msg_type {
            "user" => {
                let content = val
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .cloned()
                    .unwrap_or(Value::Null);

                // Skip tool-result messages (these are API-level "user" msgs, not human input)
                let is_tool_result = matches!(&content, Value::Array(arr)
                    if arr.iter().all(|item| item.get("type").and_then(|v| v.as_str()) == Some("tool_result")));
                if is_tool_result {
                    continue;
                }

                let text = format_content(&content);
                if !text.is_empty() {
                    println!(
                        "{} {}\n{}\n",
                        ">>>".blue().bold(),
                        timestamp.dimmed(),
                        text
                    );
                }
            }
            "assistant" => {
                let content = val
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let text = format_content(&content);
                if !text.is_empty() {
                    println!(
                        "{} {}\n{}\n",
                        "<<<".green().bold(),
                        timestamp.dimmed(),
                        text
                    );
                }
            }
            _ => {}
        }
    }
}

fn delete_session(info: &SessionInfo) {
    // Delete the .jsonl file
    fs::remove_file(&info.path).expect("failed to delete session file");

    // Delete the session directory (subagents, etc.) if it exists
    let session_subdir = info.path.with_extension("");
    if session_subdir.is_dir() {
        fs::remove_dir_all(&session_subdir).expect("failed to delete session directory");
    }

    println!("Deleted session {}", info.uuid);
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "__pycache__",
    "build",
    "dist",
    "venv",
    ".venv",
];

fn known_project_keys() -> HashSet<String> {
    let projects_dir = claude_projects_dir();
    let mut keys = HashSet::new();
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.path().is_dir() {
                keys.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    keys
}

fn walk_and_collect(
    dir: &Path,
    known_keys: &HashSet<String>,
    results: &mut Vec<PathBuf>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) {
    const MAX_DEPTH: usize = 10;
    if depth > MAX_DEPTH {
        return;
    }

    let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return;
    }

    let key = project_key(&canonical);
    if known_keys.contains(&key) {
        results.push(canonical.clone());
    }

    let Ok(entries) = fs::read_dir(&canonical) else {
        return;
    };
    let mut dirs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    dirs.sort_by_key(|e| e.file_name());

    for entry in dirs {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        walk_and_collect(&entry.path(), known_keys, results, visited, depth + 1);
    }
}

fn build_dir_tree(
    root_name: &str,
    base: &Path,
    entries: Vec<(PathBuf, Vec<SessionInfo>)>,
) -> DirNode {
    let mut root = DirNode {
        name: root_name.to_string(),
        sessions: Vec::new(),
        children: BTreeMap::new(),
    };

    for (path, sessions) in entries {
        let rel = path.strip_prefix(base).unwrap_or(&path);
        let components: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();

        if components.is_empty() {
            root.sessions = sessions;
            continue;
        }

        let mut node = &mut root;
        for comp in &components {
            node = node
                .children
                .entry(comp.clone())
                .or_insert_with(|| DirNode {
                    name: comp.clone(),
                    sessions: Vec::new(),
                    children: BTreeMap::new(),
                });
        }
        node.sessions = sessions;
    }

    root
}

fn render_tree(node: &DirNode, prefix: &str, is_last: bool, is_root: bool) {
    let session_str = if !node.sessions.is_empty() {
        let n = node.sessions.len();
        format!(" ({n} session{})", if n == 1 { "" } else { "s" })
    } else {
        String::new()
    };

    if is_root {
        println!("{}{}", node.name.bold(), session_str.dimmed());
    } else {
        let conn = if is_last { "└── " } else { "├── " };
        println!(
            "{prefix}{conn}{}{}",
            node.name.bold(),
            session_str.dimmed()
        );
    }

    let child_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{prefix}    ")
    } else {
        format!("{prefix}│   ")
    };

    let total = node.sessions.len() + node.children.len();
    let mut idx = 0;

    for s in &node.sessions {
        idx += 1;
        let conn = if idx == total { "└── " } else { "├── " };

        let ts = s
            .timestamp
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "?".into());

        let title = s
            .title
            .as_deref()
            .map(|t| format!(" ({})", t.cyan()))
            .unwrap_or_default();

        println!(
            "{child_prefix}{conn}{} {}{} [{} msgs]",
            &s.uuid[..8].yellow(),
            ts.dimmed(),
            title,
            s.message_count,
        );
    }

    let children: Vec<&DirNode> = node.children.values().collect();
    for child in &children {
        idx += 1;
        render_tree(child, &child_prefix, idx == total, false);
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if let Some(folder) = cli.folder {
                // Tree mode: scan folder recursively for sessions
                let keys = known_project_keys();
                if keys.is_empty() {
                    println!("No Claude sessions found");
                    return;
                }

                let canonical =
                    fs::canonicalize(&folder).unwrap_or_else(|_| folder.clone());
                let mut matching_dirs = Vec::new();
                let mut visited = HashSet::new();
                walk_and_collect(&canonical, &keys, &mut matching_dirs, &mut visited, 0);

                if matching_dirs.is_empty() {
                    println!("No Claude sessions found under {}", canonical.display());
                    return;
                }

                let mut entries: Vec<(PathBuf, Vec<SessionInfo>)> = Vec::new();
                for project_dir in matching_dirs {
                    let files = list_session_files(&project_dir);
                    if !files.is_empty() {
                        let mut sessions: Vec<SessionInfo> =
                            files.iter().map(|f| parse_session(f)).collect();
                        sessions.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
                        entries.push((project_dir, sessions));
                    }
                }

                if entries.is_empty() {
                    println!("No Claude sessions found under {}", canonical.display());
                    return;
                }

                let total_sessions: usize = entries.iter().map(|(_, s)| s.len()).sum();
                let total_projects = entries.len();

                let root_name = canonical.display().to_string();
                let tree = build_dir_tree(&root_name, &canonical, entries);
                println!();
                render_tree(&tree, "", true, true);
                println!(
                    "\n{} session{} across {} project{}",
                    total_sessions,
                    if total_sessions == 1 { "" } else { "s" },
                    total_projects,
                    if total_projects == 1 { "" } else { "s" },
                );
            } else {
                // List mode: show sessions for current project
                let dir = cli.dir.unwrap_or_else(|| std::env::current_dir().unwrap());
                let files = list_session_files(&dir);
                if files.is_empty() {
                    println!("No Claude sessions found for {}", dir.display());
                    return;
                }

                let mut sessions: Vec<SessionInfo> =
                    files.iter().map(|f| parse_session(f)).collect();
                sessions.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

                println!(
                    "{} session(s) for {}\n",
                    sessions.len(),
                    dir.display().to_string().dimmed()
                );

                for s in &sessions {
                    let ts = s
                        .timestamp
                        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "?".into());

                    let title = s
                        .title
                        .as_deref()
                        .map(|t| format!(" ({})", t.cyan()))
                        .unwrap_or_default();

                    let msg = s.first_message.as_deref().unwrap_or("");

                    println!(
                        "  {} {}{} [{} msgs]",
                        &s.uuid[..8].yellow(),
                        ts.dimmed(),
                        title,
                        s.message_count
                    );
                    if !msg.is_empty() {
                        println!("    {}", msg.dimmed());
                    }
                }
            }
        }
        Some(Commands::List) => {
            let dir = cli
                .folder
                .or(cli.dir)
                .unwrap_or_else(|| std::env::current_dir().unwrap());
            let files = list_session_files(&dir);
            if files.is_empty() {
                println!("No Claude sessions found for {}", dir.display());
                return;
            }

            let mut sessions: Vec<SessionInfo> =
                files.iter().map(|f| parse_session(f)).collect();
            sessions.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

            println!(
                "{} session(s) for {}\n",
                sessions.len(),
                dir.display().to_string().dimmed()
            );

            for s in &sessions {
                let ts = s
                    .timestamp
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "?".into());

                let title = s
                    .title
                    .as_deref()
                    .map(|t| format!(" ({})", t.cyan()))
                    .unwrap_or_default();

                let msg = s.first_message.as_deref().unwrap_or("");

                println!(
                    "  {} {}{} [{} msgs]",
                    &s.uuid[..8].yellow(),
                    ts.dimmed(),
                    title,
                    s.message_count
                );
                if !msg.is_empty() {
                    println!("    {}", msg.dimmed());
                }
            }
        }
        Some(Commands::Delete { session }) => {
            let dir = cli
                .folder
                .or(cli.dir)
                .unwrap_or_else(|| std::env::current_dir().unwrap());
            let files = list_session_files(&dir);
            let sessions: Vec<SessionInfo> = files.iter().map(|f| parse_session(f)).collect();
            match resolve_session(&sessions, &session) {
                Some(info) => delete_session(info),
                None => eprintln!("No session matching '{}' found", session),
            }
        }
        Some(Commands::Show { session }) => {
            let dir = cli
                .folder
                .or(cli.dir)
                .unwrap_or_else(|| std::env::current_dir().unwrap());
            let files = list_session_files(&dir);
            let sessions: Vec<SessionInfo> = files.iter().map(|f| parse_session(f)).collect();
            match resolve_session(&sessions, &session) {
                Some(info) => show_session(&info.path),
                None => eprintln!("No session matching '{}' found", session),
            }
        }
    }
}
