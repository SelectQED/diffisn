# diffisn

diffisn parses two SQL files into ASTs, and compares them based on the ASTs.

## why another diff?

Most standard diff tools rely on line-by-line text comparison, which fails spectacularly for SQL. A simple change in keyword casing, or a reordered column list can trigger dozens of false positives in a standard Git diff, masking the actual logic changes.

The name "diffisn" comes from a modified quote of the Interstellar movie:

> There is no point using energy to make another diff...
> No. It is necessary.

When your SQLs are spinning, *diff is* *n*ecessary

## Background

I develop it with AI. i write specification and test. AI does most of the coding. Not 100% "hand made", but i think it is good to share it.

Some other note:

- i am mainly working on Snowflake db (with a little bit Oracle DB)
- i use diffisn in work on a daily basis. It performs well for my purpose
- it shall work for most other dialects, but prepare for gotcha

## Features

- AST based column-by-column `CREATE TABLE` diffs
- AST based column-by-column `SELECT` diffs
- other SQL clauses (e.g. FROM, WHERE, etc.) use a token-level diff via the Patience algorithm
- **Side-by-side terminal output** — colored red/green highlights with dimmed unchanged lines
- (simple) vim-style keybindings for scrolling and navigating hunks

## Installation

Requires Rust 1.95 or above

```bash
cargo build --release
```

The binary will be at `target/release/diffisn`.

## Usage

### standalone

```bash
diffisn <old-file> <new-file>
```

### git diff

Put the platform-appropriate script from `scripts/` in your `PATH`:

| Shell | Script | Command |
|---|---|---|
| **bash / zsh** (Linux, macOS, Git Bash) | `scripts/git-diffisn` | `git diffisn` |
| **PowerShell** (Windows, VS Code) | `scripts/git-diffisn.ps1` | `git diffisn` |

Then use `git diffisn` just like the normal `git diff` command.

### TUI keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `Ctrl+d` | Page down |
| `Ctrl+u` | Page up |
| `*` | Next diff hunk |
| `#` | Previous diff hunk |
| `q` | Quit to next file |
| `Ctrl+c` | Quit entirely |

## License

[GPL-3.0](LICENSE)
