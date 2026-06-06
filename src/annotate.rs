use sqlparser::{
    ast::Statement,
    dialect::Dialect,
    parser::Parser,
    tokenizer::{Location, Token, TokenWithLocation, Tokenizer},
};

#[derive(Debug, Default)]
pub struct DiffDialect;

impl Dialect for DiffDialect {
    fn is_delimited_identifier_start(&self, ch: char) -> bool {
        ch == '"' || ch == '`'
    }

    fn is_identifier_start(&self, ch: char) -> bool {
        ch.is_alphabetic() || ch == '_' || ch == '#'
    }

    fn is_identifier_part(&self, ch: char) -> bool {
        ch.is_alphabetic()
            || ch.is_ascii_digit()
            || ch == '@'
            || ch == '$'
            || ch == '#'
            || ch == '_'
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

/// A single (non-whitespace) token with its absolute byte range in the source.
#[derive(Debug, Clone)]
pub struct TokenSpan {
    pub value: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub is_dollar_quoted: bool,
}

#[derive(Debug, Clone)]
pub enum AnnotatedItem {
    Parsed(Box<Statement>, Span),
    Raw(String, Span),
}

impl AnnotatedItem {
    pub fn span(&self) -> &Span {
        match self {
            AnnotatedItem::Parsed(_, s) => s,
            AnnotatedItem::Raw(_, s) => s,
        }
    }
}

pub fn annotate_sql(source: &str) -> Result<Vec<AnnotatedItem>, String> {
    annotate_sql_depth(source, 0)
}

fn annotate_sql_depth(source: &str, depth: usize) -> Result<Vec<AnnotatedItem>, String> {
    let line_starts = build_line_offsets(source);
    let dialect = DiffDialect {};

    let mut tokenizer = Tokenizer::new(&dialect, source);
    let all_tokens = tokenizer
        .tokenize_with_location()
        .map_err(|e| format!("Tokenization error: {}", e))?;

    let stmt_groups = split_into_statement_groups(&all_tokens);

    let mut results = Vec::new();
    for group in &stmt_groups {
        if group.is_empty() {
            continue;
        }

        let span = compute_span(&line_starts, group);

        let mut tokens_for_parser: Vec<TokenWithLocation> = group.clone();
        tokens_for_parser.push(TokenWithLocation::new(Token::EOF, 0, 0));

        let parsed = parse_all_in_group(&dialect, &tokens_for_parser);
        if parsed.is_empty() {
            let end = span.end_byte.min(source.len());
            let raw = source[span.start_byte..end].to_string();
            results.push(AnnotatedItem::Raw(raw, span));
        } else {
            for statement in parsed {
                results.push(AnnotatedItem::Parsed(Box::new(statement), span.clone()));
            }
        }
    }

    if depth == 0 {
        let bodies = collect_dollar_body_spans(source, &all_tokens, &line_starts);
        for (body_text, body_start_byte) in bodies {
            if let Ok(mut child_items) = annotate_sql_depth(&body_text, depth + 1) {
                offset_items(&mut child_items, body_start_byte);
                results.extend(child_items);
            }
        }
    }

    Ok(results)
}

fn offset_items(items: &mut [AnnotatedItem], offset: usize) {
    for item in items {
        let span = match item {
            AnnotatedItem::Parsed(_, s) => s,
            AnnotatedItem::Raw(_, s) => s,
        };
        span.start_byte += offset;
        span.end_byte += offset;
    }
}

/// Scan tokenized output for `DollarQuotedString` tokens and return
/// `(body_source_text, absolute_byte_offset)` for each body.
fn collect_dollar_body_spans(
    _source: &str,
    tokens: &[TokenWithLocation],
    line_starts: &[usize],
) -> Vec<(String, usize)> {
    let mut bodies = Vec::new();

    for tok in tokens {
        if let Token::DollarQuotedString(dqs) = &tok.token {
            let tok_start = location_to_byte(line_starts, &tok.location);
            let prefix_len: usize = match &dqs.tag {
                Some(tag) => 1 + tag.len() + 1, // $tag$
                None => 2,                       // $$
            };
            let body_start = tok_start + prefix_len;
            bodies.push((dqs.value.clone(), body_start));
        }
    }

    bodies
}

/// Returns all non-whitespace tokens from `source` with their absolute byte positions.
pub fn tokenize_with_spans(source: &str) -> Vec<TokenSpan> {
    let line_starts = build_line_offsets(source);
    let dialect = DiffDialect {};
    let mut tokenizer = Tokenizer::new(&dialect, source);
    let all_tokens = tokenizer.tokenize_with_location().unwrap_or_default();

    all_tokens
        .iter()
        .filter(|t| !matches!(t.token, Token::Whitespace(_) | Token::EOF))
        .map(|t| {
            let value = t.token.to_string();
            let start_byte = location_to_byte(&line_starts, &t.location);
            // Use the raw source slice to get accurate byte length
            let end_byte = (start_byte + value.len()).min(source.len());
            TokenSpan {
                value,
                start_byte,
                end_byte,
                is_dollar_quoted: matches!(t.token, Token::DollarQuotedString(..)),
            }
        })
        .collect()
}

fn parse_all_in_group(
    dialect: &DiffDialect,
    tokens: &[TokenWithLocation],
) -> Vec<Statement> {
    Parser::new(dialect)
        .with_tokens_with_locations(tokens.to_vec())
        .parse_statements()
        .unwrap_or_default()
}

fn compute_span(line_starts: &[usize], tokens: &[TokenWithLocation]) -> Span {
    let first_tok = &tokens[0];
    let last_tok = &tokens[tokens.len() - 1];

    let start_byte = location_to_byte(line_starts, &first_tok.location);
    let last_display = last_tok.token.to_string();
    let last_width = last_display.chars().count();
    let end_byte = location_to_byte(line_starts, &last_tok.location) + last_width;

    Span {
        start_line: first_tok.location.line as usize,
        start_col: first_tok.location.column as usize,
        end_line: last_tok.location.line as usize,
        end_col: last_tok.location.column as usize + last_width.saturating_sub(1),
        start_byte,
        end_byte,
    }
}

pub fn build_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    for (i, ch) in source.char_indices() {
        if ch == '\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

pub fn location_to_byte(line_starts: &[usize], loc: &Location) -> usize {
    let line = loc.line as usize;
    let col = loc.column as usize;
    if line == 0 || col == 0 {
        return 0;
    }
    if line <= line_starts.len() {
        line_starts[line - 1] + col.saturating_sub(1)
    } else {
        0
    }
}

fn split_into_statement_groups(tokens: &[TokenWithLocation]) -> Vec<Vec<TokenWithLocation>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();

    for token in tokens {
        match &token.token {
            Token::SemiColon => {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            }
            Token::EOF => {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(token.clone());
            }
        }
    }

    if !current.is_empty() {
        groups.push(current);
    }

    groups
}
