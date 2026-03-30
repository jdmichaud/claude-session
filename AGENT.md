# Agent Guide

## Project

`cs` is a small Rust CLI that reads Claude Code session files (JSONL) from `~/.claude/projects/` and presents them in the terminal. No network access, no API keys.

## Build & Test

```
cargo build              # debug build
cargo build --release    # release build
cargo clippy             # lint (must pass with zero warnings)
```

There are no tests yet. Verify changes manually:

```
cargo run                # list sessions for cwd
cargo run -- ~/          # tree view from home
cargo run -- show <id>   # display a conversation
```

## Architecture

Single-file project: `src/main.rs`. Key sections:

- **CLI parsing** (clap derive): `Cli` struct + `Commands` enum at the top
- **Session discovery**: `project_key()` maps a directory path to the key Claude Code uses under `~/.claude/projects/`; `list_session_files()` finds JSONL files there
- **Session parsing**: `parse_session()` reads a JSONL file and extracts metadata into `SessionInfo`
- **Tree view**: `walk_and_collect()` recursively walks a directory tree, checking each subdirectory against known project keys; `build_dir_tree()` and `render_tree()` build and display the ASCII tree
- **Show**: `show_session()` / `format_content()` render a full conversation to stdout

## Conventions

- Edition 2024, no unsafe
- `cargo clippy` must pass cleanly
- Keep it a single file unless there is a strong reason to split
- Skip hidden directories and common noise dirs (node_modules, target, vendor, etc.) during tree walks
