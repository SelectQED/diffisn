use crate::annotate::{build_line_offsets, Span};
use crate::diff::DiffResult;
use colored::*;
use similar::{Algorithm, capture_diff_slices, DiffOp};
use std::env;

pub struct RenderedLine {
    pub left: String,
    pub is_change: bool,
    pub right: String,
}

pub struct RenderedDiff {
    pub lines: Vec<RenderedLine>,
    pub diff_indices: Vec<usize>,
}

pub fn render_diff(
    file_path: &str,
    old_source: &str,
    new_source: &str,
    diffs: &[DiffResult],
) -> RenderedDiff {
    let term_width = get_terminal_width();
    let gutter = 5; // " │ "
    let col_width = (term_width.saturating_sub(gutter)) / 2;

    let mut result = RenderedDiff {
        lines: Vec::new(),
        diff_indices: Vec::new(),
    };

    let left_label = format!("--- a/{}", file_path);
    let right_label = format!("+++ b/{}", file_path);
    result.lines.push(RenderedLine {
        left: pad_right(&left_label, col_width).red().bold().to_string(),
        is_change: false,
        right: pad_right(&right_label, col_width).green().bold().to_string(),
    });

    let old_line_starts = build_line_offsets(old_source);
    let new_line_starts = build_line_offsets(new_source);

    for diff in diffs {
        match diff {
            DiffResult::Unchanged { old_span, .. } => {
                render_unchanged(old_source, old_span, col_width, &mut result.lines);
            }
            DiffResult::Modified {
                old_span,
                new_span,
                old_changed,
                new_changed,
            } => {
                render_modified(
                    old_source,
                    new_source,
                    old_span,
                    new_span,
                    old_changed,
                    new_changed,
                    &old_line_starts,
                    &new_line_starts,
                    col_width,
                    &mut result.lines,
                );
            }
            DiffResult::Deleted { old_span } => {
                let text = extract_text(old_source, old_span);
                let bg = Color::TrueColor { r: 255, g: 200, b: 215 };
                let fg = Color::TrueColor { r: 200, g: 20, b: 20 };
                for line in text.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    for (chunk, _) in wrap_line(line, 0, col_width) {
                        result.lines.push(RenderedLine {
                            left: pad_right(chunk, col_width).color(fg).on_color(bg).to_string(),
                            is_change: true,
                            right: pad_right("", col_width).on_color(bg).to_string(),
                        });
                    }
                }
            }
            DiffResult::Inserted { new_span } => {
                let text = extract_text(new_source, new_span);
                let bg = Color::TrueColor { r: 205, g: 245, b: 205 };
                let fg = Color::TrueColor { r: 0, g: 130, b: 0 };
                for line in text.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    for (chunk, _) in wrap_line(line, 0, col_width) {
                        result.lines.push(RenderedLine {
                            left: pad_right("", col_width).on_color(bg).to_string(),
                            is_change: true,
                            right: pad_right(chunk, col_width).color(fg).on_color(bg).to_string(),
                        });
                    }
                }
            }
        }
    }

    // Build perfect hunk indices based on actual rendered changed lines
    let mut in_block = false;
    for (idx, line) in result.lines.iter().enumerate() {
        if line.is_change {
            if !in_block {
                result.diff_indices.push(idx);
                in_block = true;
            }
        } else {
            in_block = false;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

fn wrap_line<'a>(line: &'a str, mut line_start: usize, col_width: usize) -> Vec<(&'a str, usize)> {
    if line.is_empty() {
        return vec![("", line_start)];
    }
    let mut chunks = Vec::new();
    let mut current = line;
    while current.chars().count() > col_width {
        let mut byte_end = 0;
        let mut char_count = 0;
        for c in current.chars() {
            if char_count == col_width {
                break;
            }
            byte_end += c.len_utf8();
            char_count += 1;
        }
        chunks.push((&current[..byte_end], line_start));
        current = &current[byte_end..];
        line_start += byte_end;
    }
    if !current.is_empty() {
        chunks.push((current, line_start));
    }
    chunks
}

fn render_unchanged(source: &str, span: &Span, col_width: usize, lines: &mut Vec<RenderedLine>) {
    let text = extract_text(source, span);
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        for (chunk, _) in wrap_line(line, 0, col_width) {
            let styled = pad_right(chunk, col_width).dimmed().to_string();
            lines.push(RenderedLine {
                left: styled.clone(),
                is_change: false,
                right: styled,
            });
        }
    }
}

fn render_side_by_side_lines(
    old_lines: &[(&str, usize)],
    new_lines: &[(&str, usize)],
    old_changed: &[(usize, usize)],
    new_changed: &[(usize, usize)],
    col_width: usize,
    lines: &mut Vec<RenderedLine>,
) {
    let old_vals: Vec<&str> = old_lines.iter().map(|(l, _)| *l).collect();
    let new_vals: Vec<&str> = new_lines.iter().map(|(l, _)| *l).collect();
    
    let ops = capture_diff_slices(Algorithm::Myers, &old_vals, &new_vals);
    
    for op in ops {
        match op {
            DiffOp::Equal { old_index, new_index, len } => {
                for i in 0..len {
                    let left = old_lines.get(old_index + i).copied();
                    let right = new_lines.get(new_index + i).copied();
                    push_side_by_side_chunk(left, right, old_changed, new_changed, col_width, lines);
                }
            }
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..old_len {
                    let left = old_lines.get(old_index + i).copied();
                    push_side_by_side_chunk(left, None, old_changed, new_changed, col_width, lines);
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..new_len {
                    let right = new_lines.get(new_index + i).copied();
                    push_side_by_side_chunk(None, right, old_changed, new_changed, col_width, lines);
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let max_len = old_len.max(new_len);
                for i in 0..max_len {
                    let left = if i < old_len { old_lines.get(old_index + i).copied() } else { None };
                    let right = if i < new_len { new_lines.get(new_index + i).copied() } else { None };
                    push_side_by_side_chunk(left, right, old_changed, new_changed, col_width, lines);
                }
            }
        }
    }
}

fn push_side_by_side_chunk(
    left: Option<(&str, usize)>,
    right: Option<(&str, usize)>,
    old_changed: &[(usize, usize)],
    new_changed: &[(usize, usize)],
    col_width: usize,
    lines: &mut Vec<RenderedLine>,
) {
    let left_chunks = left.map(|(l, off)| wrap_line(l, off, col_width)).unwrap_or_default();
    let right_chunks = right.map(|(l, off)| wrap_line(l, off, col_width)).unwrap_or_default();

    let chunk_max = left_chunks.len().max(right_chunks.len()).max(1);
    for j in 0..chunk_max {
        let l_chunk = left_chunks.get(j).copied();
        let r_chunk = right_chunks.get(j).copied();

        let (left_col, left_has_change) = match l_chunk {
            Some((line, off)) if !line.is_empty() || (j == 0 && left.unwrap().0.is_empty()) => {
                color_line_padded(line, off, old_changed, Color::Red, col_width)
            }
            _ => (" ".repeat(col_width), false),
        };

        let (right_col, right_has_change) = match r_chunk {
            Some((line, off)) if !line.is_empty() || (j == 0 && right.unwrap().0.is_empty()) => {
                color_line_padded(line, off, new_changed, Color::Green, col_width)
            }
            _ => (" ".repeat(col_width), false),
        };

        lines.push(RenderedLine {
            left: left_col,
            is_change: left_has_change || right_has_change,
            right: right_col,
        });
    }
}

fn render_modified(
    old_source: &str,
    new_source: &str,
    old_span: &Span,
    new_span: &Span,
    old_changed: &[(usize, usize)],
    new_changed: &[(usize, usize)],
    old_line_starts: &[usize],
    new_line_starts: &[usize],
    col_width: usize,
    lines: &mut Vec<RenderedLine>,
) {
    let old_text = extract_text(old_source, old_span);
    let new_text = extract_text(new_source, new_span);

    let has_dollar = old_text.contains("$$") || new_text.contains("$$");

    if has_dollar {
        render_modified_collapsed(old_text, new_text, old_changed, new_changed,
                                   old_span.start_byte, new_span.start_byte, col_width, lines);
        return;
    }

    let old_lines = lines_with_offsets(old_text, old_span.start_byte, old_line_starts);
    let new_lines = lines_with_offsets(new_text, new_span.start_byte, new_line_starts);
    render_side_by_side_lines(&old_lines, &new_lines, old_changed, new_changed, col_width, lines);
}

fn render_modified_collapsed(
    old_text: &str,
    new_text: &str,
    old_changed: &[(usize, usize)],
    new_changed: &[(usize, usize)],
    old_start: usize,
    new_start: usize,
    col_width: usize,
    lines: &mut Vec<RenderedLine>,
) {
    let old_lines = collapse_dollar_lines(old_text, old_start);
    let new_lines = collapse_dollar_lines(new_text, new_start);

    let old_refs: Vec<(&str, usize)> = old_lines.iter().map(|(l, off)| (l.as_str(), *off)).collect();
    let new_refs: Vec<(&str, usize)> = new_lines.iter().map(|(l, off)| (l.as_str(), *off)).collect();
    render_side_by_side_lines(&old_refs, &new_refs, old_changed, new_changed, col_width, lines);
}

/// Split text into lines, collapsing any `$$...$$` body content to `$$…$$`.
fn collapse_dollar_lines(text: &str, text_start: usize) -> Vec<(String, usize)> {
    let collapsed = collapse_dollar_bodies(text);
    let mut result = Vec::new();
    let mut byte_pos = text_start;
    for line in collapsed.split('\n') {
        result.push((line.to_string(), byte_pos));
        byte_pos += line.len() + 1;
    }
    if result.last().map(|(l, _)| l.is_empty()).unwrap_or(false) {
        result.pop();
    }
    result
}

fn collapse_dollar_bodies(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(dollar_start) = rest.find("$$") {
        out.push_str(&rest[..dollar_start + 2]);
        rest = &rest[dollar_start + 2..];
        if let Some(dollar_end) = rest.find("$$") {
            out.push('…');
            out.push_str(&rest[dollar_end..dollar_end + 2]);
            rest = &rest[dollar_end + 2..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

/// Split `text` into `(line_str, absolute_byte_offset)` pairs.
/// `text_start` is the byte offset of `text[0]` within its source file.
fn lines_with_offsets<'a>(
    text: &'a str,
    text_start: usize,
    _line_starts: &[usize],
) -> Vec<(&'a str, usize)> {
    let mut result = Vec::new();
    let mut byte_pos = text_start;
    for line in text.split('\n') {
        result.push((line, byte_pos));
        byte_pos += line.len() + 1; // +1 for the '\n'
    }
    // Drop trailing empty entry if the text ended with '\n'
    if result.last().map(|(l, _)| l.is_empty()).unwrap_or(false) {
        result.pop();
    }
    result
}

/// Render `line` with inline coloring: characters whose byte position falls
/// inside any `changed_ranges` are colored with `color`; the rest are dimmed.
/// The result is padded (with plain spaces) to `col_width` visible characters.
fn color_line_padded(
    line: &str,
    line_start: usize,
    changed_ranges: &[(usize, usize)],
    color: Color,
    col_width: usize,
) -> (String, bool) {
    let char_count = line.chars().count();

    let mut line_has_change = false;
    let mut byte_pos = line_start;
    for ch in line.chars() {
        if changed_ranges.iter().any(|(s, e)| byte_pos >= *s && byte_pos < *e) {
            line_has_change = true;
            break;
        }
        byte_pos += ch.len_utf8();
    }

    let bg = if !line_has_change {
        None
    } else {
        Some(match color {
            Color::Red => Color::TrueColor { r: 255, g: 200, b: 215 },
            Color::Green => Color::TrueColor { r: 205, g: 245, b: 205 },
            _ => return (build_colored_segments(line, line_start, changed_ranges, color, None), false),
        })
    };

    let fg = match color {
        Color::Red => Color::TrueColor { r: 200, g: 20, b: 20 },
        Color::Green => Color::TrueColor { r: 0, g: 130, b: 0 },
        _ => color,
    };

    let colored = build_colored_segments(line, line_start, changed_ranges, fg, bg);

    if char_count >= col_width {
        return (colored, line_has_change);
    }

    let padding = col_width.saturating_sub(char_count);
    let result = match bg {
        Some(b) => format!("{}{}", colored, " ".repeat(padding).on_color(b).to_string()),
        None => format!("{}{}", colored, " ".repeat(padding)),
    };
    (result, line_has_change)
}


/// Build a string where each character segment is either dimmed or colored,
/// based on whether its byte position falls within `changed_ranges`.
fn build_colored_segments(
    text: &str,
    text_start: usize,
    changed_ranges: &[(usize, usize)],
    change_color: Color,
    bg: Option<Color>,
) -> String {
    if changed_ranges.is_empty() {
        let s = text.dimmed();
        return match bg {
            Some(b) => s.on_color(b).to_string(),
            None => s.to_string(),
        };
    }

    // Walk chars, group into (segment_text, is_changed) runs.
    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut byte_pos = text_start;
    let mut seg = String::new();
    let mut seg_changed = false;

    for ch in text.chars() {
        let ch_changed = changed_ranges
            .iter()
            .any(|(s, e)| byte_pos >= *s && byte_pos < *e);

        if segments.is_empty() && seg.is_empty() {
            // First character — initialise segment state.
            seg_changed = ch_changed;
        }

        if ch_changed != seg_changed {
            // Flush current segment.
            if !seg.is_empty() {
                segments.push((seg.clone(), seg_changed));
                seg.clear();
            }
            seg_changed = ch_changed;
        }

        seg.push(ch);
        byte_pos += ch.len_utf8();
    }
    if !seg.is_empty() {
        segments.push((seg, seg_changed));
    }

    let mut out = String::new();
    for (s, is_changed) in segments {
        let styled = if is_changed {
            s.color(change_color)
        } else {
            s.dimmed()
        };
        out.push_str(&match bg {
            Some(b) => styled.on_color(b).to_string(),
            None => styled.to_string(),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

fn get_terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120)
}

fn pad_right(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let display_width = s.chars().count();
    if display_width >= width {
        s.to_string()
    } else {
        format!("{:<width$}", s, width = width)
    }
}

fn extract_text<'a>(source: &'a str, span: &Span) -> &'a str {
    let end = span.end_byte.min(source.len());
    &source[span.start_byte..end]
}
