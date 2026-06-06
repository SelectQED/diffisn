use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use std::fs;

#[derive(Debug, Default)]
struct DiffDialect;

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

fn main() {
    let source = fs::read_to_string("payee_query.sql").unwrap();
    let dialect = DiffDialect {};

    let statements = match Parser::parse_sql(&dialect, &source) {
        Ok(stmts) => stmts,
        Err(e) => {
            println!("Parse error: {:?}", e);
            return;
        }
    };

    for (i, stmt) in statements.iter().enumerate() {
        println!("--- AST FOR LHS PAYEE_TAX_CODE (Statement {}) ---", i);
        println!("{:#?}", stmt);
    }
}
