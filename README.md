# diffisn

Instead of a line-by-line text diff, diffisn parses SQL into an AST, and compare based on AST.

## Features

- column-by-column `CREATE TABLE` diffs
- column-by-column `SELECT` diffs
- other sql clauses (e.g. from clause, where clause, etc) are using a simpler cmoparison TODO: what is it?
- **Side-by-side terminal output** — colored red/green highlights with dimmed unchanged lines
- (simple) vim-style keybindings for scrolling and navigating hunks

## Installation

Requires Rust 1.95+.

```bash
cargo build --release
```

The binary will be at `target/release/diffisn`.

## Usage

### Manual mode

```bash
diffisn <old-file> <new-file>
diffisn -v <old-file> <new-file>    # verbose debug output
``

### Git mode

Configure `.gitconfig`:

```

[diff "sqldiff"]
    command = diffisn

```

Add to `.gitattributes`:

```

*.sql diff=sqldiff

```

Or use directly:

```bash
GIT_EXTERNAL_DIFF=diffisn git diff
```

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
