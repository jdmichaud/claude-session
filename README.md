# cs

A small CLI to browse and manage [Claude Code](https://claude.ai/code) sessions.

Claude Code stores conversations as JSONL files under `~/.claude/projects/`.
`cs` reads them directly — no API keys, no network access.

## Usage

Run `cs` from any directory where you've used Claude Code:

```
$ cs
3 session(s) for /home/user/myproject

  db1e7abd 2026-03-27 06:16 (my-session) [44 msgs]
    write a tool in rust that...
  ea3b5d0b 2026-02-25 19:04 [1108 msgs]
    Analyze the file main.c and try to identify...
  f9537a6e 2026-03-18 18:53 (refactor-auth) [740 msgs]
    How would you approach...
```

### Commands

```
cs                      # list sessions (default)
cs list                 # same as above
cs show <id>            # print conversation to stdout
cs delete <id>          # delete a session
```

Sessions can be referenced by UUID prefix or title prefix:

```
cs show db1e            # by UUID prefix
cs show my-session      # by title (set with /rename in Claude Code)
cs show my              # by title prefix
```

### Options

```
-d, --dir <PATH>        # target a different project directory
```

## Install

### From source

```
cargo install --path .
```

### Static binary (Linux x86_64)

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/cs ~/.local/bin/
```

Pre-built static binaries are available on the [releases](../../releases) page.

## License

MIT
