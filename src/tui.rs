use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, ClearType, DisableLineWrap, EnableLineWrap},
};
use colored::*;
use std::io::{self, IsTerminal, Write};
use std::fs::File;
use std::os::unix::io::AsRawFd;

pub fn run_pager(lines: &[crate::output::RenderedLine], diff_indices: &[usize]) -> io::Result<()> {
    // Reattach stdin to the TTY if Git hijacked it
    if !io::stdin().is_terminal() {
        if let Ok(tty) = File::open("/dev/tty") {
            unsafe {
                libc::dup2(tty.as_raw_fd(), libc::STDIN_FILENO);
            }
        }
    }

    // Reattach stdout to the TTY if Git hijacked it
    if !io::stdout().is_terminal() {
        if let Ok(tty) = File::options().write(true).open("/dev/tty") {
            unsafe {
                libc::dup2(tty.as_raw_fd(), libc::STDOUT_FILENO);
            }
        }
    }

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide, DisableLineWrap)?;

    let mut num_buf = String::new();
    let mut active_diff_idx: Option<usize> = diff_indices.first().copied();

    let (_cols, term_rows) = terminal::size()?;
    let initial_visible = (term_rows as usize).saturating_sub(1);
    let mut scroll = active_diff_idx
        .map(|i| i.saturating_sub(initial_visible / 2))
        .unwrap_or(0);

    loop {
        let (term_cols, term_rows) = terminal::size()?;
        let cols = term_cols as usize;
        let rows = term_rows as usize;

        let visible_rows = rows.saturating_sub(1);
        let max_scroll = lines.len().saturating_sub(visible_rows);

        // Render Frame - line by line to prevent flicker and wrap messiness
        for i in 0..visible_rows {
            queue!(stdout, cursor::MoveTo(0, i as u16), terminal::Clear(ClearType::CurrentLine))?;
            if let Some(line) = lines.get(scroll + i) {
                let line_idx = scroll + i;
                let is_active = Some(line_idx) == active_diff_idx;
                let sep = if is_active {
                    " \\ "
                        .color(Color::TrueColor { r: 120, g: 90, b: 0 })
                        .on_color(Color::TrueColor { r: 255, g: 245, b: 170 })
                        .bold()
                        .to_string()
                } else if line.is_change {
                    " \\ ".yellow().bold().to_string()
                } else {
                    " │ ".dimmed().to_string()
                };
                let formatted = format!("{} {} {}", line.left, sep, line.right);
                queue!(stdout, Print(formatted))?;
            }
        }

        // Render beautiful status bar on the last row
        let status_row = rows.saturating_sub(1);
        queue!(stdout, cursor::MoveTo(0, status_row as u16), terminal::Clear(ClearType::CurrentLine))?;

        let status_text = format!(
            "  DIFFISN │ q: Quit │ j/k: Scroll │ */#: Jump Diff │ Line {}/{} │ {}",
            scroll + 1,
            lines.len(),
            if num_buf.is_empty() {
                "".to_string()
            } else {
                format!("Go to: {}", num_buf)
            }
        );

        // Pad status bar to cols - 1 to absolutely guarantee no line wrap in bottom-right corner
        let padded_status = format!("{:width$}", status_text, width = cols.saturating_sub(1));
        let status_styled = padded_status.black().on_color(Color::TrueColor { r: 220, g: 220, b: 220 }).to_string();
        queue!(stdout, Print(status_styled))?;

        stdout.flush()?;

        // Input Event Loop
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press { continue; }

            let mult = if num_buf.is_empty() { 1 } else { num_buf.parse::<usize>().unwrap_or(1) };

            match key.code {
                KeyCode::Char('q') => break, // Gracefully proceed to the next file in git diff
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Forcefully abort the ENTIRE `git diff` process loop!
                    let _ = terminal::disable_raw_mode();
                    let _ = execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show, EnableLineWrap);
                    unsafe { libc::kill(0, libc::SIGINT); }
                    std::process::exit(130);
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    scroll = scroll.saturating_add(mult).min(max_scroll);
                    num_buf.clear();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll = scroll.saturating_sub(mult);
                    num_buf.clear();
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll = scroll.saturating_add((visible_rows / 2) * mult).min(max_scroll);
                    num_buf.clear();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll = scroll.saturating_sub((visible_rows / 2) * mult);
                    num_buf.clear();
                }
                KeyCode::Char('*') => {
                    let current_focus = active_diff_idx.unwrap_or(scroll);
                    if active_diff_idx.is_some()
                        && (current_focus < scroll || current_focus >= scroll + visible_rows)
                    {
                        scroll = current_focus.saturating_sub(visible_rows / 2).min(max_scroll);
                    } else if let Some(&next_idx) = diff_indices.iter().find(|&&idx| idx > current_focus)
                    {
                        active_diff_idx = Some(next_idx);
                        if next_idx < scroll + 2 || next_idx >= scroll + visible_rows.saturating_sub(2) {
                            scroll = next_idx.saturating_sub(visible_rows / 2).min(max_scroll);
                        }
                    }
                    num_buf.clear();
                }
                KeyCode::Char('#') => {
                    let current_focus = active_diff_idx.unwrap_or(scroll);
                    if let Some(&prev_idx) = diff_indices.iter().rev().find(|&&idx| idx < current_focus) {
                        active_diff_idx = Some(prev_idx);
                        if prev_idx < scroll + 2 || prev_idx >= scroll + visible_rows.saturating_sub(2) {
                            scroll = prev_idx.saturating_sub(visible_rows / 2).min(max_scroll);
                        }
                    }
                    num_buf.clear();
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    num_buf.push(c);
                }
                _ => num_buf.clear(),
            }
        }
    }

    // Cleanup terminal state before exiting
    execute!(stdout, EnableLineWrap, terminal::LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
