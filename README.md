# diffisn

diffisn parses two SQL files into ASTs, and compares them based on the ASTs.

## Background

The code is mostly written by AI. But i think it is good to share it anyway.

## Features

- column-by-column `CREATE TABLE` diffs
- column-by-column `SELECT` diffs
- other SQL clauses (e.g. FROM, WHERE, etc.) use a token-level diff via the Patience algorithm
- **Side-by-side terminal output** — colored red/green highlights with dimmed unchanged lines
- (simple) vim-style keybindings for scrolling and navigating hunks

## Other Note

- i am mainly working on Snowflake db (with a little bit Oracle DB)
- it seems to work with Snowflake SQL dialect
- not sure how it works with other SQL dialects

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

put scripts/git-diffisn shell script file in path

and then you can use `git diffisn` command

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
