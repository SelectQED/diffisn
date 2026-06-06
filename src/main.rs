mod annotate;
mod diff;
mod output;
mod tui;

use annotate::annotate_sql;
use diff::diff_items;
use colored::Colorize;
use output::render_diff;
use std::env;
use std::fs;
use std::io::IsTerminal;
use std::process;

macro_rules! vlog {
    ($verbose:expr, $($arg:tt)*) => {
        if $verbose {
            eprintln!($($arg)*);
        }
    };
}

fn main() {
    unsafe {
        std::env::set_var("COLORTERM", "truecolor");
    }
    colored::control::set_override(true);

    let verbose = env::var("DIFFISN_VERBOSE").is_ok_and(|v| !v.is_empty() && v != "0");

    let mut args: Vec<String> = env::args().collect();

    let flag_verbose = if args.len() == 4
        && (args[1] == "-v" || args[1] == "--verbose")
    {
        args.remove(1);
        true
    } else {
        false
    };
    let verbose = verbose || flag_verbose;

    vlog!(verbose, "[verbose] args ({}) = {:?}", args.len(), args);

    let (file_path, old_file, new_file, is_git_mode) = if args.len() == 8 {
        let p = (args[1].clone(), args[2].clone(), args[5].clone(), true);
        vlog!(verbose, "[verbose] git mode: path={}, old={}, new={}", p.0, p.1, p.2);
        p
    } else if args.len() == 3 {
        let p = ("Manual Diff".to_string(), args[1].clone(), args[2].clone(), false);
        vlog!(verbose, "[verbose] manual mode: old={}, new={}", p.0, p.1);
        p
    } else {
        eprintln!("Usage:");
        eprintln!("  Git mode: diffisn <path> <old-file> <old-hex> <old-mode> <new-file> <new-hex> <new-mode>");
        eprintln!("  Manual:   diffisn [-v] <old-file> <new-file>");
        process::exit(1);
    };

    let old_source = fs::read_to_string(&old_file).unwrap_or_default();
    let new_source = fs::read_to_string(&new_file).unwrap_or_default();

    vlog!(verbose, "[verbose] old file '{}': {} bytes", old_file, old_source.len());
    vlog!(verbose, "[verbose] new file '{}': {} bytes", new_file, new_source.len());

    if old_source.is_empty() && new_source.is_empty() {
        vlog!(verbose, "[verbose] both files empty, nothing to diff");
        process::exit(0);
    }

    let old_items = match annotate_sql(&old_source) {
        Ok(items) => {
            vlog!(verbose, "[verbose] old: tokenized {} statements", items.len());
            if verbose {
                for (i, item) in items.iter().enumerate() {
                    let span = item.span();
                    match item {
                        annotate::AnnotatedItem::Parsed(stmt, _) => {
                            vlog!(verbose, "[verbose]   old[{}]: Parsed({}) span=[{}-{}]", i, stmt.to_string().chars().take(80).collect::<String>(), span.start_byte, span.end_byte);
                        }
                        annotate::AnnotatedItem::Raw(s, _) => {
                            vlog!(verbose, "[verbose]   old[{}]: Raw({:?}) span=[{}-{}]", i, &s[..s.len().min(60)], span.start_byte, span.end_byte);
                        }
                    }
                }
            }
            items
        }
        Err(e) => {
            eprintln!("{}: tokenization error (old): {}", old_file, e);
            process::exit(1);
        }
    };
    let new_items = match annotate_sql(&new_source) {
        Ok(items) => {
            vlog!(verbose, "[verbose] new: tokenized {} statements", items.len());
            if verbose {
                for (i, item) in items.iter().enumerate() {
                    let span = item.span();
                    match item {
                        annotate::AnnotatedItem::Parsed(stmt, _) => {
                            vlog!(verbose, "[verbose]   new[{}]: Parsed({}) span=[{}-{}]", i, stmt.to_string().chars().take(80).collect::<String>(), span.start_byte, span.end_byte);
                        }
                        annotate::AnnotatedItem::Raw(s, _) => {
                            vlog!(verbose, "[verbose]   new[{}]: Raw({:?}) span=[{}-{}]", i, &s[..s.len().min(60)], span.start_byte, span.end_byte);
                        }
                    }
                }
            }
            items
        }
        Err(e) => {
            eprintln!("{}: tokenization error (new): {}", new_file, e);
            process::exit(1);
        }
    };

    let diffs = diff_items(&old_items, &new_items, &old_source, &new_source);

    let unchanged = diffs.iter().filter(|d| matches!(d, diff::DiffResult::Unchanged { .. })).count();
    let modified = diffs.iter().filter(|d| matches!(d, diff::DiffResult::Modified { .. })).count();
    let deleted = diffs.iter().filter(|d| matches!(d, diff::DiffResult::Deleted { .. })).count();
    let inserted = diffs.iter().filter(|d| matches!(d, diff::DiffResult::Inserted { .. })).count();
    vlog!(verbose, "[verbose] diff results: unchanged={}, modified={}, deleted={}, inserted={}",
          unchanged, modified, deleted, inserted);

    let has_changes = diffs.iter().any(|d| !matches!(d, diff::DiffResult::Unchanged { .. }));

    if !has_changes {
        vlog!(verbose, "[verbose] no AST changes detected, exiting 0");
        process::exit(0);
    }

    let rendered_diff = render_diff(&file_path, &old_source, &new_source, &diffs);
    
    // If we are piped (Git pager is active), it is UNSAFE to launch crossterm because it fights with `less`.
    if is_git_mode && !std::io::stdout().is_terminal() {
        // We print a helpful hint at the top of the file diff!
        println!("{}", "╭──────────────────────────────────────────────────────────────────╮".color(colored::Color::TrueColor { r: 100, g: 100, b: 100 }));
        println!("{} {}", "│".color(colored::Color::TrueColor { r: 100, g: 100, b: 100 }), "Hint: Git pager detected. Interactive TUI disabled to prevent terminal corruption.".yellow());
        println!("{} {}", "│".color(colored::Color::TrueColor { r: 100, g: 100, b: 100 }), "To enable the full TUI with status bar and */# navigation, run:".yellow());
        println!("{} {}", "│".color(colored::Color::TrueColor { r: 100, g: 100, b: 100 }), "GIT_PAGER=cat GIT_EXTERNAL_DIFF=diffisn git diff".bright_green().bold());
        println!("{}", "╰──────────────────────────────────────────────────────────────────╯\n".color(colored::Color::TrueColor { r: 100, g: 100, b: 100 }));
        
        for line in rendered_diff.lines {
            println!("{} │ {}", line.left, line.right);
        }
    } else {
        if let Err(e) = tui::run_pager(&rendered_diff.lines, &rendered_diff.diff_indices) {
            eprintln!("TUI Error: {}", e);
        }
    }
}
