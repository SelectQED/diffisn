use crate::annotate::{AnnotatedItem, Span, TokenSpan, tokenize_with_spans};
use similar::{Algorithm, capture_diff_slices, DiffOp};
use sqlparser::ast::{ColumnDef, Statement, Query, TableFactor};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum DiffResult {
    Unchanged {
        #[allow(dead_code)]
        old_span: Span,
        #[allow(dead_code)]
        new_span: Span,
    },
    /// A statement exists on both sides but differs.
    /// `old_changed` / `new_changed` are the absolute byte ranges (within the
    /// respective source string) that correspond to changed tokens.
    Modified {
        old_span: Span,
        new_span: Span,
        old_changed: Vec<(usize, usize)>,
        new_changed: Vec<(usize, usize)>,
    },
    Deleted {
        old_span: Span,
    },
    Inserted {
        new_span: Span,
    },
}

/// Semantic change descriptors produced by AST-level comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AstDiff {
    /// A column was added to a table.
    ColumnAdded { col_name: String },
    /// A column was dropped from a table.
    ColumnDropped { col_name: String },
    /// A column's definition (type, constraints, etc.) changed.
    ColumnModified { col_name: String },
    /// Fallback: statement type not yet supported for deep semantic diffing.
    UnsupportedMacroChange,
}

fn item_key(item: &AnnotatedItem) -> String {
    match item {
        AnnotatedItem::Parsed(stmt, _) => format!("{:?}", stmt),
        AnnotatedItem::Raw(s, _) => s.clone(),
    }
}

/// Returns an identity string for DDL statements that name a specific entity.
/// The identity is `"TYPE:name"` (e.g. `"TABLE:users"`, `"VIEW:active_users"`).
/// DML statements (SELECT, INSERT, etc.) and unparseable fragments return `None`.
fn item_identity(item: &AnnotatedItem) -> Option<String> {
    match item {
        AnnotatedItem::Parsed(stmt, _) => match stmt.as_ref() {
            Statement::CreateTable { name, .. } => Some(format!("TABLE:{}", name)),
            Statement::CreateView { name, .. } => Some(format!("VIEW:{}", name)),
            Statement::CreateIndex { name: Some(name), .. } => Some(format!("INDEX:{}", name)),
            Statement::CreateFunction { name, .. } => Some(format!("FUNCTION:{}", name)),
            Statement::CreateProcedure { name, .. } => Some(format!("PROCEDURE:{}", name)),
            Statement::CreateSequence { name, .. } => Some(format!("SEQUENCE:{}", name)),
            Statement::CreateType { name, .. } => Some(format!("TYPE:{}", name)),
            Statement::CreateSchema { schema_name, .. } => Some(format!("SCHEMA:{}", schema_name)),
            Statement::CreateDatabase { db_name, .. } => Some(format!("DATABASE:{}", db_name)),
            Statement::CreateVirtualTable { name, .. } => Some(format!("VIRTUAL_TABLE:{}", name)),
            Statement::CreateRole { names, .. } if !names.is_empty() => Some(format!("ROLE:{}", names[0])),
            Statement::CreateExtension { name, .. } => Some(format!("EXTENSION:{}", name)),
            Statement::AlterTable { name, .. } => Some(format!("TABLE:{}", name)),
            Statement::AlterView { name, .. } => Some(format!("VIEW:{}", name)),
            Statement::AlterIndex { name, .. } => Some(format!("INDEX:{}", name)),
            Statement::AlterRole { name, .. } => Some(format!("ROLE:{}", name)),
            Statement::Drop { object_type, names, .. } if !names.is_empty() => {
                Some(format!("{}:{}", object_type, names[0]))
            }
            Statement::DropFunction { func_desc, .. } if !func_desc.is_empty() => {
                Some(format!("FUNCTION:{}", func_desc[0].name))
            }
            _ => None,
        },
        AnnotatedItem::Raw(_, _) => None,
    }
}

pub fn diff_items(
    old_items: &[AnnotatedItem],
    new_items: &[AnnotatedItem],
    old_source: &str,
    new_source: &str,
) -> Vec<DiffResult> {
    let mut old_matched: Vec<Option<usize>> = vec![None; old_items.len()];
    let mut new_matched: Vec<Option<usize>> = vec![None; new_items.len()];

    let mut old_by_id: HashMap<String, Vec<usize>> = HashMap::new();
    let mut new_by_id: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, item) in old_items.iter().enumerate() {
        if let Some(id) = item_identity(item) {
            old_by_id.entry(id).or_default().push(i);
        }
    }
    for (i, item) in new_items.iter().enumerate() {
        if let Some(id) = item_identity(item) {
            new_by_id.entry(id).or_default().push(i);
        }
    }

    let all_ids: Vec<String> = old_by_id.keys().cloned().collect();
    for id in &all_ids {
        let old_indices = old_by_id.get(id).cloned().unwrap_or_default();
        let new_indices = new_by_id.get(id).cloned().unwrap_or_default();
        let max_pairs = old_indices.len().min(new_indices.len());

        for k in 0..max_pairs {
            old_matched[old_indices[k]] = Some(new_indices[k]);
            new_matched[new_indices[k]] = Some(old_indices[k]);
        }
    }

    let old_remaining: Vec<usize> = (0..old_items.len())
        .filter(|i| old_matched[*i].is_none() && item_identity(&old_items[*i]).is_none())
        .collect();
    let new_remaining: Vec<usize> = (0..new_items.len())
        .filter(|i| new_matched[*i].is_none() && item_identity(&new_items[*i]).is_none())
        .collect();

    if !old_remaining.is_empty() || !new_remaining.is_empty() {
        let old_sigs: Vec<_> = old_remaining
            .iter()
            .map(|&idx| {
                let item = &old_items[idx];
                let tokens = tokenize_span(item.span(), old_source);
                extract_signature(&tokens, item.span(), old_source)
            })
            .collect();

        let new_sigs: Vec<_> = new_remaining
            .iter()
            .map(|&idx| {
                let item = &new_items[idx];
                let tokens = tokenize_span(item.span(), new_source);
                extract_signature(&tokens, item.span(), new_source)
            })
            .collect();

        let mut candidates = Vec::new();
        for (i, &old_idx) in old_remaining.iter().enumerate() {
            for (j, &new_idx) in new_remaining.iter().enumerate() {
                let score = compute_similarity(&old_sigs[i], &new_sigs[j]);
                if score >= 0.35 {
                    candidates.push((score, i, j, old_idx, new_idx));
                }
            }
        }

        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut old_paired = vec![false; old_remaining.len()];
        let mut new_paired = vec![false; new_remaining.len()];

        for (_, i, j, old_idx, new_idx) in candidates {
            if !old_paired[i] && !new_paired[j] {
                old_paired[i] = true;
                new_paired[j] = true;

                old_matched[old_idx] = Some(new_idx);
                new_matched[new_idx] = Some(old_idx);
            }
        }
    }

    let mut results = Vec::new();
    let mut new_emitted = vec![false; new_items.len()];
    let mut new_cursor: usize = 0;

    for (old_idx, old_item) in old_items.iter().enumerate() {
        match old_matched[old_idx] {
            Some(new_idx) => {
                while new_cursor < new_idx {
                    if new_matched[new_cursor].is_none() && !new_emitted[new_cursor] {
                        results.push(DiffResult::Inserted {
                            new_span: new_items[new_cursor].span().clone(),
                        });
                        new_emitted[new_cursor] = true;
                    }
                    new_cursor += 1;
                }

                new_emitted[new_idx] = true;

                results.extend(diff_statement_pair(
                    old_item,
                    &new_items[new_idx],
                    old_source,
                    new_source,
                ));

                if new_cursor <= new_idx {
                    new_cursor = new_idx + 1;
                }
            }
            None => {
                results.push(DiffResult::Deleted {
                    old_span: old_item.span().clone(),
                });
            }
        }
    }

    for ni in new_cursor..new_items.len() {
        if new_matched[ni].is_none() && !new_emitted[ni] {
            results.push(DiffResult::Inserted {
                new_span: new_items[ni].span().clone(),
            });
        }
    }

    results
}

fn is_sql_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "select" | "from" | "join" | "on" | "where" | "and" | "or" | "not" | "in" |
        "is" | "null" | "case" | "when" | "then" | "else" | "end" | "group" | "by" | "order" |
        "having" | "limit" | "offset" | "left" | "right" | "inner" | "outer" | "full" | "cross" |
        "union" | "all" | "distinct" | "cast" | "like" | "ilike" | "true" | "false" | "exists" |
        "between" | "create" | "table" | "replace" | "insert" | "update" | "delete" | "into" |
        "values" | "procedure" | "function" | "view" | "index" | "schema" | "database" | "role"
    )
}

fn extract_signature(
    tokens: &[TokenSpan],
    span: &Span,
    source: &str,
) -> (HashSet<String>, HashSet<String>, HashSet<String>, HashSet<String>) {
    let mut tables = HashSet::new();
    let mut columns = HashSet::new();
    let mut token_bag = HashSet::new();
    
    for t in tokens {
        let val = t.value.to_lowercase();
        if val.chars().all(|c| c.is_alphanumeric() || c == '_') && val.len() > 1 {
            token_bag.insert(val);
        }
    }
    
    let mut i = 0;
    while i < tokens.len() {
        let val = tokens[i].value.to_lowercase();
        if val == "from" || val == "join" || val == "into" || val == "update" {
            let mut table_name = String::new();
            let mut j = i + 1;
            while j < tokens.len() {
                let next_val = &tokens[j].value;
                if next_val.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '"') {
                    table_name.push_str(next_val);
                    j += 1;
                } else {
                    break;
                }
            }
            if !table_name.is_empty() {
                tables.insert(table_name.to_lowercase());
            }
            i = j;
            continue;
        }
        
        if val == "select" {
            let mut j = i + 1;
            while j < tokens.len() {
                let next_val = tokens[j].value.to_lowercase();
                if next_val == "from" {
                    break;
                }
                if next_val.chars().all(|c| c.is_alphanumeric() || c == '_') && next_val.len() > 1 {
                    if !is_sql_keyword(&next_val) {
                        columns.insert(next_val);
                    }
                }
                j += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    
    let mut word_bag = HashSet::new();
    let raw_text = &source[span.start_byte..span.end_byte];
    for word in raw_text.split_whitespace() {
        let w = word.to_lowercase();
        let trimmed: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
        if trimmed.len() > 2 {
            word_bag.insert(trimmed);
        }
    }
    
    (tables, columns, token_bag, word_bag)
}

fn compute_similarity(
    old_sig: &(HashSet<String>, HashSet<String>, HashSet<String>, HashSet<String>),
    new_sig: &(HashSet<String>, HashSet<String>, HashSet<String>, HashSet<String>),
) -> f64 {
    let (old_tables, old_cols, old_tokens, old_words) = old_sig;
    let (new_tables, new_cols, new_tokens, new_words) = new_sig;
    
    let table_score = if old_tables.is_empty() && new_tables.is_empty() {
        0.0
    } else {
        let intersection: HashSet<_> = old_tables.intersection(new_tables).collect();
        let union: HashSet<_> = old_tables.union(new_tables).collect();
        if union.is_empty() {
            0.0
        } else {
            (intersection.len() as f64) / (union.len() as f64)
        }
    };
    
    let col_score = if old_cols.is_empty() && new_cols.is_empty() {
        0.0
    } else {
        let intersection: HashSet<_> = old_cols.intersection(new_cols).collect();
        let union: HashSet<_> = old_cols.union(new_cols).collect();
        if union.is_empty() {
            0.0
        } else {
            (intersection.len() as f64) / (union.len() as f64)
        }
    };
    
    let token_score = if old_tokens.is_empty() && new_tokens.is_empty() {
        0.0
    } else {
        let intersection: HashSet<_> = old_tokens.intersection(new_tokens).collect();
        let union: HashSet<_> = old_tokens.union(new_tokens).collect();
        if union.is_empty() {
            0.0
        } else {
            (intersection.len() as f64) / (union.len() as f64)
        }
    };
    
    let word_score = if old_words.is_empty() && new_words.is_empty() {
        0.0
    } else {
        let intersection: HashSet<_> = old_words.intersection(new_words).collect();
        let union: HashSet<_> = old_words.union(new_words).collect();
        if union.is_empty() {
            0.0
        } else {
            (intersection.len() as f64) / (union.len() as f64)
        }
    };
    
    let mut total_weight = 0.0;
    let mut weighted_sum = 0.0;
    
    if !old_tables.is_empty() || !new_tables.is_empty() {
        weighted_sum += table_score * 0.5;
        total_weight += 0.5;
    }
    if !old_cols.is_empty() || !new_cols.is_empty() {
        weighted_sum += col_score * 0.35;
        total_weight += 0.35;
    }
    if !old_tokens.is_empty() || !new_tokens.is_empty() {
        weighted_sum += token_score * 0.15;
        total_weight += 0.15;
    }
    
    if total_weight == 0.0 {
        return word_score;
    }
    
    weighted_sum / total_weight
}

// ---------------------------------------------------------------------------
// Intra-statement semantic diffing
// ---------------------------------------------------------------------------

/// Compare two parsed statements at the AST level, producing a set of
/// semantic change descriptors.  Returns `UnsupportedMacroChange` for
/// statement types that don't have deep-traversal logic yet.
fn compare_ast_nodes(old: &AnnotatedItem, new: &AnnotatedItem) -> Vec<AstDiff> {
    let (old_stmt, new_stmt) = match (old, new) {
        (AnnotatedItem::Parsed(o, _), AnnotatedItem::Parsed(n, _)) => (o, n),
        _ => return vec![AstDiff::UnsupportedMacroChange],
    };

    match (old_stmt.as_ref(), new_stmt.as_ref()) {
        (Statement::CreateTable { .. }, Statement::CreateTable { .. }) => {
            let mut old_clone = old_stmt.as_ref().clone();
            let mut new_clone = new_stmt.as_ref().clone();
            
            if let (Statement::CreateTable { columns: old_cols, .. }, Statement::CreateTable { columns: new_cols, .. }) = (&mut old_clone, &mut new_clone) {
                let o_cols = std::mem::take(old_cols);
                let n_cols = std::mem::take(new_cols);
                
                // If any field other than columns (e.g. query, constraints, engine) differs, fallback to flat token diff
                if old_clone != new_clone {
                    return vec![AstDiff::UnsupportedMacroChange];
                }
                
                diff_table_columns(&o_cols, &n_cols)
            } else {
                vec![AstDiff::UnsupportedMacroChange]
            }
        }
        _ => vec![AstDiff::UnsupportedMacroChange],
    }
}

/// Column-level semantic diff for `CREATE TABLE` statements.
/// Uses the same HashMap identity trick: columns are matched by name
/// regardless of position, so reordered columns produce no diff.
fn diff_table_columns(
    old_cols: &[ColumnDef],
    new_cols: &[ColumnDef],
) -> Vec<AstDiff> {
    let mut diffs = Vec::new();

    let old_map: HashMap<&str, &ColumnDef> = old_cols
        .iter()
        .map(|c| (c.name.value.as_str(), c))
        .collect();
    let new_map: HashMap<&str, &ColumnDef> = new_cols
        .iter()
        .map(|c| (c.name.value.as_str(), c))
        .collect();

    for (name, new_col) in &new_map {
        match old_map.get(name) {
            Some(old_col) => {
                if old_col != new_col {
                    diffs.push(AstDiff::ColumnModified {
                        col_name: name.to_string(),
                    });
                }
            }
            None => {
                diffs.push(AstDiff::ColumnAdded {
                    col_name: name.to_string(),
                });
            }
        }
    }

    for name in old_map.keys() {
        if !new_map.contains_key(name) {
            diffs.push(AstDiff::ColumnDropped {
                col_name: name.to_string(),
            });
        }
    }

    diffs
}

/// Produce a single `DiffResult` for a pair of statements that are known to
/// differ.  Tries semantic AST diffing first; falls back to the flat token
/// diff for unsupported statement types.
fn diff_statement_pair(
    old: &AnnotatedItem,
    new: &AnnotatedItem,
    old_source: &str,
    new_source: &str,
) -> Vec<DiffResult> {
    let old_span = old.span().clone();
    let new_span = new.span().clone();

    if item_key(old) == item_key(new) {
        return vec![DiffResult::Unchanged { old_span, new_span }];
    }

    if let (AnnotatedItem::Parsed(old_stmt, _), AnnotatedItem::Parsed(new_stmt, _)) = (old, new) {
        if let Some(diffs) = diff_hybrid_query(old_stmt, new_stmt, &old_span, &new_span, old_source, new_source) {
            return diffs;
        }
    }

    let semantic_diffs = compare_ast_nodes(old, new);

    if semantic_diffs.is_empty() {
        return vec![DiffResult::Unchanged { old_span, new_span }];
    }

    let (old_changed, new_changed) =
        if semantic_diffs.iter().any(|d| matches!(d, AstDiff::UnsupportedMacroChange)) {
            compute_changed_ranges(&old_span, &new_span, old_source, new_source)
        } else {
            map_ast_diffs_to_bytes(&semantic_diffs, &old_span, &new_span, old_source, new_source)
        };

    vec![DiffResult::Modified {
        old_span,
        new_span,
        old_changed,
        new_changed,
    }]
}

fn table_factor_name(factor: &TableFactor) -> String {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            if let Some(a) = alias {
                a.name.value.clone()
            } else {
                name.to_string()
            }
        }
        TableFactor::Derived { alias, .. } => {
            if let Some(a) = alias {
                a.name.value.clone()
            } else {
                "".to_string()
            }
        }
        _ => "".to_string(),
    }
}

fn diff_hybrid_query(
    old_stmt: &Statement,
    new_stmt: &Statement,
    old_span: &Span,
    new_span: &Span,
    old_source: &str,
    new_source: &str,
) -> Option<Vec<DiffResult>> {
    let old_q = extract_query(old_stmt)?;
    let new_q = extract_query(new_stmt)?;
    
    let old_tokens = tokenize_span(old_span, old_source);
    let new_tokens = tokenize_span(new_span, new_source);
    
    let old_froms = extract_froms(&old_q)?;
    let new_froms = extract_froms(&new_q)?;
    if old_froms.len() != 1 || new_froms.len() != 1 { return None; }
    
    let old_chunks = chunk_tokens(&old_tokens, &old_froms[0]);
    let new_chunks = chunk_tokens(&new_tokens, &new_froms[0]);
    
    let mut diffs = Vec::new();
    
    diffs.extend(compare_field_chunks(&old_chunks.select, &new_chunks.select));
    if let Some(d) = compare_chunks(old_chunks.from, new_chunks.from) { diffs.push(d); }
    
    let mut old_joins_map: HashMap<String, TokenChunk> = old_chunks.joins.into_iter().collect();
    let mut new_joins_map: HashMap<String, TokenChunk> = new_chunks.joins.into_iter().collect();
    
    let mut all_join_keys: Vec<String> = old_joins_map.keys().chain(new_joins_map.keys()).cloned().collect();
    all_join_keys.sort();
    all_join_keys.dedup();
    
    for key in all_join_keys {
        let o = old_joins_map.remove(&key);
        let n = new_joins_map.remove(&key);
        if let Some(d) = compare_chunks(o, n) { diffs.push(d); }
    }
    
    if let Some(d) = compare_chunks(old_chunks.other, new_chunks.other) { diffs.push(d); }
    
    Some(diffs)
}

struct TokenChunk<'a> {
    span: Span,
    tokens: &'a [TokenSpan],
}

struct ChunkList<'a> {
    chunks: Vec<TokenChunk<'a>>,
}

struct QueryChunks<'a> {
    select: ChunkList<'a>,
    from: Option<TokenChunk<'a>>,
    joins: Vec<(String, TokenChunk<'a>)>,
    other: Option<TokenChunk<'a>>,
}

fn extract_query(stmt: &Statement) -> Option<Query> {
    match stmt {
        Statement::CreateTable { query: Some(q), .. } => Some(*q.clone()),
        Statement::Query(q) => Some(*q.clone()),
        _ => None,
    }
}

fn extract_froms(q: &Query) -> Option<Vec<sqlparser::ast::TableWithJoins>> {
    if let sqlparser::ast::SetExpr::Select(sel) = q.body.as_ref() {
        Some(sel.from.clone())
    } else {
        None
    }
}

fn find_outer_keyword(tokens: &[TokenSpan], keyword: &str) -> Option<usize> {
    let mut depth: i32 = 0;
    for (i, t) in tokens.iter().enumerate() {
        if t.value == "(" { depth += 1; }
        if t.value == ")" { depth = depth.saturating_sub(1); }
        if depth == 0 && t.value.eq_ignore_ascii_case(keyword) {
            return Some(i);
        }
    }
    None
}

fn make_chunk<'a>(tokens: &'a [TokenSpan]) -> Option<TokenChunk<'a>> {
    if tokens.is_empty() {
        None
    } else {
        Some(TokenChunk {
            span: Span {
                start_byte: tokens.first().unwrap().start_byte,
                end_byte: tokens.last().unwrap().end_byte,
                start_line: 0,
                start_col: 0,
                end_line: 0,
                end_col: 0,
            },
            tokens,
        })
    }
}

fn split_by_depth0_commas<'a>(tokens: &'a [TokenSpan]) -> Vec<&'a [TokenSpan]> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut depth: i32 = 0;
    for (i, t) in tokens.iter().enumerate() {
        if t.value == "(" { depth += 1; }
        if t.value == ")" { depth = depth.saturating_sub(1); }
        if depth == 0 && t.value == "," {
            fields.push(&tokens[start..i]);
            start = i + 1;
        }
    }
    if start < tokens.len() {
        fields.push(&tokens[start..]);
    }
    fields
}

fn make_chunk_list<'a>(fields: &[&'a [TokenSpan]]) -> ChunkList<'a> {
    let chunks: Vec<TokenChunk<'a>> = fields.iter().filter_map(|f| make_chunk(f)).collect();
    ChunkList { chunks }
}

fn chunk_tokens<'a>(tokens: &'a [TokenSpan], from_table: &sqlparser::ast::TableWithJoins) -> QueryChunks<'a> {
    let from_idx = find_outer_keyword(tokens, "FROM").unwrap_or(tokens.len());
    
    let where_idx = find_outer_keyword(tokens, "WHERE")
        .or_else(|| find_outer_keyword(tokens, "GROUP"))
        .or_else(|| find_outer_keyword(tokens, "ORDER"))
        .or_else(|| find_outer_keyword(tokens, "HAVING"))
        .or_else(|| find_outer_keyword(tokens, "LIMIT"))
        .unwrap_or(tokens.len());
        
    let select_tokens = &tokens[..from_idx];
    let select_fields = split_by_depth0_commas(select_tokens);
    let select_chunks = make_chunk_list(&select_fields);
    
    let mut join_indices = Vec::new();
    let mut depth: i32 = 0;
    for (i, t) in tokens.iter().enumerate().skip(from_idx) {
        if i >= where_idx { break; }
        if t.value == "(" { depth += 1; }
        if t.value == ")" { depth = depth.saturating_sub(1); }
        if depth == 0 && t.value.to_uppercase() == "JOIN" {
            let mut start = i;
            while start > from_idx {
                let prev = tokens[start - 1].value.to_uppercase();
                if matches!(prev.as_str(), "INNER" | "LEFT" | "RIGHT" | "OUTER" | "FULL" | "CROSS") {
                    start -= 1;
                } else {
                    break;
                }
            }
            join_indices.push(start);
        }
    }
    
    let from_end = join_indices.first().copied().unwrap_or(where_idx);
    let from_tokens = &tokens[from_idx..from_end];
    
    let mut joins = Vec::new();
    for (i, &start_idx) in join_indices.iter().enumerate() {
        let end_idx = join_indices.get(i + 1).copied().unwrap_or(where_idx);
        let join_toks = &tokens[start_idx..end_idx];
        
        if i < from_table.joins.len() {
            let name = table_factor_name(&from_table.joins[i].relation);
            if let Some(chunk) = make_chunk(join_toks) {
                joins.push((name, chunk));
            }
        }
    }
    
    let other_tokens = &tokens[where_idx..];
    
    QueryChunks {
        select: select_chunks,
        from: make_chunk(from_tokens),
        joins,
        other: make_chunk(other_tokens),
    }
}

fn compare_field_chunks(
    old_list: &ChunkList,
    new_list: &ChunkList,
) -> Vec<DiffResult> {
    let old_keys: Vec<String> = old_list.chunks.iter().map(|c| {
        c.tokens.iter().map(|t| t.value.as_str()).collect::<Vec<_>>().join("")
    }).collect();
    let new_keys: Vec<String> = new_list.chunks.iter().map(|c| {
        c.tokens.iter().map(|t| t.value.as_str()).collect::<Vec<_>>().join("")
    }).collect();

    let ops = capture_diff_slices(Algorithm::Patience, &old_keys, &new_keys);
    let mut diffs = Vec::new();

    for op in &ops {
        match op {
            DiffOp::Equal { old_index, new_index, len } => {
                for i in 0..*len {
                    let o = &old_list.chunks[old_index + i];
                    let n = &new_list.chunks[new_index + i];
                    diffs.push(DiffResult::Unchanged {
                        old_span: o.span.clone(),
                        new_span: n.span.clone(),
                    });
                }
            }
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..*old_len {
                    let o = &old_list.chunks[old_index + i];
                    diffs.push(DiffResult::Deleted {
                        old_span: o.span.clone(),
                    });
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..*new_len {
                    let n = &new_list.chunks[new_index + i];
                    diffs.push(DiffResult::Inserted {
                        new_span: n.span.clone(),
                    });
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let max_len = (*old_len).max(*new_len);
                for i in 0..max_len {
                    let o = if i < *old_len { Some(&old_list.chunks[old_index + i]) } else { None };
                    let n = if i < *new_len { Some(&new_list.chunks[new_index + i]) } else { None };
                    match (o, n) {
                        (Some(oc), Some(nc)) => {
                            let old_vals: Vec<&str> = oc.tokens.iter().map(|t| t.value.as_str()).collect();
                            let new_vals: Vec<&str> = nc.tokens.iter().map(|t| t.value.as_str()).collect();
                            if old_vals == new_vals {
                                diffs.push(DiffResult::Unchanged {
                                    old_span: oc.span.clone(),
                                    new_span: nc.span.clone(),
                                });
                            } else {
                                let mut o_ch = Vec::new();
                                let mut n_ch = Vec::new();
                                let token_ops = capture_diff_slices(Algorithm::Patience, &old_vals, &new_vals);
                                for top in token_ops {
                                    match top {
                                        DiffOp::Delete { old_index: oi, old_len: ol, .. } => {
                                            for j in 0..ol { o_ch.push((oc.tokens[oi + j].start_byte, oc.tokens[oi + j].end_byte)); }
                                        }
                                        DiffOp::Insert { new_index: ni, new_len: nl, .. } => {
                                            for j in 0..nl { n_ch.push((nc.tokens[ni + j].start_byte, nc.tokens[ni + j].end_byte)); }
                                        }
                                        DiffOp::Replace { old_index: oi, old_len: ol, new_index: ni, new_len: nl } => {
                                            for j in 0..ol { o_ch.push((oc.tokens[oi + j].start_byte, oc.tokens[oi + j].end_byte)); }
                                            for j in 0..nl { n_ch.push((nc.tokens[ni + j].start_byte, nc.tokens[ni + j].end_byte)); }
                                        }
                                        _ => {}
                                    }
                                }
                                diffs.push(DiffResult::Modified {
                                    old_span: oc.span.clone(),
                                    new_span: nc.span.clone(),
                                    old_changed: o_ch,
                                    new_changed: n_ch,
                                });
                            }
                        }
                        (Some(oc), None) => {
                            diffs.push(DiffResult::Deleted { old_span: oc.span.clone() });
                        }
                        (None, Some(nc)) => {
                            diffs.push(DiffResult::Inserted { new_span: nc.span.clone() });
                        }
                        (None, None) => {}
                    }
                }
            }
        }
    }

    diffs
}

fn compare_chunks(
    old_c: Option<TokenChunk>,
    new_c: Option<TokenChunk>,
) -> Option<DiffResult> {
    match (old_c, new_c) {
        (Some(o), Some(n)) => {
            let old_vals: Vec<&str> = o.tokens.iter().map(|t| t.value.as_str()).collect();
            let new_vals: Vec<&str> = n.tokens.iter().map(|t| t.value.as_str()).collect();
            
            if old_vals == new_vals {
                Some(DiffResult::Unchanged { old_span: o.span, new_span: n.span })
            } else {
                let mut o_ch = Vec::new();
                let mut n_ch = Vec::new();
                let ops = capture_diff_slices(Algorithm::Patience, &old_vals, &new_vals);
                for op in ops {
                    match op {
                        DiffOp::Delete { old_index, old_len, .. } => {
                            for i in 0..old_len {
                                let t = &o.tokens[old_index + i];
                                o_ch.push((t.start_byte, t.end_byte));
                            }
                        }
                        DiffOp::Insert { new_index, new_len, .. } => {
                            for i in 0..new_len {
                                let t = &n.tokens[new_index + i];
                                n_ch.push((t.start_byte, t.end_byte));
                            }
                        }
                        DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                            for i in 0..old_len {
                                let t = &o.tokens[old_index + i];
                                o_ch.push((t.start_byte, t.end_byte));
                            }
                            for i in 0..new_len {
                                let t = &n.tokens[new_index + i];
                                n_ch.push((t.start_byte, t.end_byte));
                            }
                        }
                        _ => {}
                    }
                }
                Some(DiffResult::Modified {
                    old_span: o.span,
                    new_span: n.span,
                    old_changed: o_ch,
                    new_changed: n_ch,
                })
            }
        }
        (Some(o), None) => Some(DiffResult::Deleted { old_span: o.span }),
        (None, Some(n)) => Some(DiffResult::Inserted { new_span: n.span }),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Anchor-based byte-range mapping
// ---------------------------------------------------------------------------

/// Convert a list of semantic `AstDiff` descriptors into concrete byte ranges
/// for highlighting.  Uses the token stream to locate column names and their
/// surrounding definitions.
fn map_ast_diffs_to_bytes(
    diffs: &[AstDiff],
    old_span: &Span,
    new_span: &Span,
    old_source: &str,
    new_source: &str,
) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let mut old_changed: Vec<(usize, usize)> = Vec::new();
    let mut new_changed: Vec<(usize, usize)> = Vec::new();

    let old_tokens = tokenize_span(old_span, old_source);
    let new_tokens = tokenize_span(new_span, new_source);

    for diff in diffs {
        match diff {
            AstDiff::ColumnModified { col_name } => {
                if let Some(range) = column_range(&old_tokens, col_name) {
                    old_changed.push(range);
                }
                if let Some(range) = column_range(&new_tokens, col_name) {
                    new_changed.push(range);
                }
            }
            AstDiff::ColumnAdded { col_name } => {
                if let Some(range) = column_range(&new_tokens, col_name) {
                    new_changed.push(range);
                }
            }
            AstDiff::ColumnDropped { col_name } => {
                if let Some(range) = column_range(&old_tokens, col_name) {
                    old_changed.push(range);
                }
            }
            AstDiff::UnsupportedMacroChange => {
                unreachable!("UnsupportedMacroChange should be caught before map_ast_diffs_to_bytes");
            }
        }
    }

    let old_dollar = dollar_ranges(&old_tokens);
    let new_dollar = dollar_ranges(&new_tokens);
    old_changed.retain(|r| !old_dollar.contains(r));
    new_changed.retain(|r| !new_dollar.contains(r));

    (old_changed, new_changed)
}

/// Tokenize a span, returning tokens with absolute byte positions.
fn tokenize_span(span: &Span, source: &str) -> Vec<TokenSpan> {
    let end = span.end_byte.min(source.len());
    let sub = &source[span.start_byte..end];
    tokenize_with_spans(sub)
        .into_iter()
        .map(|t| TokenSpan {
            start_byte: t.start_byte + span.start_byte,
            end_byte: t.end_byte + span.start_byte,
            ..t
        })
        .collect()
}

/// Find the byte range of a column definition anchored by `col_name` within
/// the token stream.  Scans from the name token to the next `,` or `)` at
/// depth 0 (tracking parentheses for constraint expressions like
/// `CHECK (expr)`).
fn column_range(tokens: &[TokenSpan], col_name: &str) -> Option<(usize, usize)> {
    let start_idx = tokens.iter().position(|t| t.value.eq_ignore_ascii_case(col_name))?;
    let start_byte = tokens[start_idx].start_byte;

    let mut depth: i32 = 0;
    let mut end_byte = tokens.last().map(|t| t.end_byte).unwrap_or(start_byte);

    for t in tokens.iter().skip(start_idx + 1) {
        match t.value.as_str() {
            "(" => depth += 1,
            ")" if depth == 0 => {
                end_byte = t.start_byte;
                break;
            }
            ")" => depth -= 1,
            "," if depth == 0 => {
                end_byte = t.start_byte;
                break;
            }
            _ => {}
        }
    }

    Some((start_byte, end_byte))
}

fn dollar_ranges(tokens: &[TokenSpan]) -> Vec<(usize, usize)> {
    tokens
        .iter()
        .filter(|t| t.is_dollar_quoted)
        .map(|t| (t.start_byte, t.end_byte))
        .collect()
}

// ---------------------------------------------------------------------------
// Fallback: flat Myers token diff (used when semantic diffing is unsupported)
// ---------------------------------------------------------------------------

/// Tokenize both statement slices, run a Myers diff on the token values,
/// and return the byte ranges of the changed tokens in each source.
fn compute_changed_ranges(
    old_span: &Span,
    new_span: &Span,
    old_source: &str,
    new_source: &str,
) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let old_tokens = tokenize_span(old_span, old_source);
    let new_tokens = tokenize_span(new_span, new_source);

    let old_vals: Vec<&str> = old_tokens.iter().map(|t| t.value.as_str()).collect();
    let new_vals: Vec<&str> = new_tokens.iter().map(|t| t.value.as_str()).collect();

    let ops = capture_diff_slices(Algorithm::Patience, &old_vals, &new_vals);

    let mut old_changed: Vec<(usize, usize)> = Vec::new();
    let mut new_changed: Vec<(usize, usize)> = Vec::new();

    for op in &ops {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_index, old_len, .. } => {
                for tok in &old_tokens[*old_index..*old_index + *old_len] {
                    old_changed.push((tok.start_byte, tok.end_byte));
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for tok in &new_tokens[*new_index..*new_index + *new_len] {
                    new_changed.push((tok.start_byte, tok.end_byte));
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                for tok in &old_tokens[*old_index..*old_index + *old_len] {
                    old_changed.push((tok.start_byte, tok.end_byte));
                }
                for tok in &new_tokens[*new_index..*new_index + *new_len] {
                    new_changed.push((tok.start_byte, tok.end_byte));
                }
            }
        }
    }

    let old_dollar = dollar_ranges(&old_tokens);
    let new_dollar = dollar_ranges(&new_tokens);
    old_changed.retain(|r| !old_dollar.contains(r));
    new_changed.retain(|r| !new_dollar.contains(r));

    (old_changed, new_changed)
}
