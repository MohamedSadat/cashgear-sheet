//! Where a plain formula reads one cell of a range.
//!
//! A file's plain `<f>` follows the rules from before dynamic arrays. A function's parameter
//! takes a single value, a reference, or an array, and that decides what a range does there:
//! `LEFT(A1:A9,1)` reads the cell of `A1:A9` level with the formula, `SUM(A1:A9)` reads them all,
//! and `SUM(A1:A9*2)` reads one, because the operator wants values. A parameter that takes an array
//! puts what is under it in array mode, so `SUMPRODUCT(A1:A9*2)` reads them all. `IF`'s condition
//! and a few others like it stay out of array mode, which is why `SUMPRODUCT(IF(A1:A9>0,1))` needs
//! array entry in Excel. Excel writes the first kind as `@` once it knows about dynamic arrays, so
//! that is what the engine gets.

use formualizer_eval::array_params::{param_class, ParamClass};
use formualizer_parse::parser::{
    parse, ASTNode, ASTNodeType, BinaryOperator, ReferenceType, TableSpecifier,
};
use formualizer_parse::{LiteralValue, UnaryOperator};

/// What is expected where an expression stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wants {
    Value,
    Reference,
    /// Array mode: nothing is cut down.
    Array,
}

impl Wants {
    fn of(self, param: ParamClass) -> Self {
        match (param, self) {
            (ParamClass::Scalar, _) => Wants::Value,
            (ParamClass::Reference, Wants::Array) => Wants::Array,
            (ParamClass::Reference, _) => Wants::Reference,
            // `IF`'s branches give what `IF` gives, but leave array mode behind.
            (ParamClass::Branch, Wants::Value) => Wants::Value,
            (ParamClass::Branch, _) => Wants::Reference,
            (ParamClass::Array, _) | (ParamClass::Value, Wants::Array) => Wants::Array,
            (ParamClass::Value, _) => Wants::Value,
        }
    }

    fn of_operator(self) -> Self {
        match self {
            Wants::Array => Wants::Array,
            _ => Wants::Value,
        }
    }
}

/// Whether a reference stands for more than one cell.
fn is_many(reference: &ReferenceType, many_names: &dyn Fn(&str) -> bool) -> bool {
    match reference {
        ReferenceType::Range {
            start_row,
            start_col,
            end_row,
            end_col,
            ..
        } => {
            start_row.is_none()
                || start_col.is_none()
                || start_row != end_row
                || start_col != end_col
        }
        ReferenceType::NamedRange(name) => many_names(name),
        // `T[Column]` is the whole column, but `T[@Column]` is already one row.
        ReferenceType::Table(table) => !matches!(
            &table.specifier,
            Some(TableSpecifier::Row(_) | TableSpecifier::SpecialItem(_))
        ),
        _ => false,
    }
}

/// Whether a function call can give several values where only one is wanted. `ROW` and `COLUMN` of
/// a range give an array, of which a plain formula only sees the first, even in `SUM(ROW(A1:A9))`.
/// `OFFSET` and `INDIRECT` give a reference, which is cut down like any other.
fn gives_many(name: &str, args: &[ASTNode], wants: Wants, names: &dyn Fn(&str) -> bool) -> bool {
    let is_many_arg = |arg: Option<&ASTNode>| {
        arg.is_some_and(|arg| match &arg.node_type {
            ASTNodeType::Reference { reference, .. } => is_many(reference, names),
            _ => false,
        })
    };
    match name {
        "ROW" | "COLUMN" => wants != Wants::Array && is_many_arg(args.first()),
        "OFFSET" => {
            wants == Wants::Value
                && (args.len() > 3 || is_many_arg(args.first()))
                && !args[3.min(args.len())..].iter().all(|arg| {
                    matches!(&arg.node_type, ASTNodeType::Literal(value)
                        if matches!(value, LiteralValue::Number(n) if *n == 1.0)
                            || matches!(value, LiteralValue::Int(1)))
                })
        }
        "INDIRECT" => wants == Wants::Value,
        _ => false,
    }
}

fn walk(node: &ASTNode, wants: Wants, names: &dyn Fn(&str) -> bool, marks: &mut Vec<usize>) {
    match &node.node_type {
        ASTNodeType::Reference { reference, .. } => {
            if wants == Wants::Value && is_many(reference, names) {
                if let Some(token) = &node.source_token {
                    marks.push(token.start);
                }
            }
        }
        ASTNodeType::UnaryOp { op, expr } => {
            // What was written with `@` or `#` says what it means.
            if !matches!(
                op,
                UnaryOperator::ImplicitIntersection | UnaryOperator::Spill
            ) {
                walk(expr, wants.of_operator(), names, marks);
            }
        }
        ASTNodeType::BinaryOp { op, left, right } => {
            if !matches!(
                op,
                BinaryOperator::Range | BinaryOperator::Union | BinaryOperator::Intersection
            ) {
                walk(left, wants.of_operator(), names, marks);
                walk(right, wants.of_operator(), names, marks);
            }
        }
        ASTNodeType::Function { name, args } => {
            if gives_many(name, args, wants, names) {
                if let Some(token) = &node.source_token {
                    marks.push(token.start);
                }
            }
            for (at, arg) in args.iter().enumerate() {
                if let Some(param) = param_class(name, at) {
                    walk(arg, wants.of(param), names, marks);
                }
            }
        }
        _ => {}
    }
}

/// `text` with `@` before each range that a plain formula reads one cell of, if there are any. The
/// rest of the text stays as written. `many_names` says whether a defined name is several cells.
pub(crate) fn mark(text: &str, many_names: &dyn Fn(&str) -> bool) -> Option<String> {
    // Parsing is the cost here, and it can only be worth it when something can be cut down.
    let can_be_many = text.contains(':')
        || text.contains('[')
        || text.contains('{')
        || text.contains("OFFSET(")
        || text.contains("INDIRECT(")
        || mentions_name(text, many_names);
    if !can_be_many {
        return None;
    }
    let source = format!("={text}");
    let ast = parse(&source).ok()?;
    let mut marks = Vec::new();
    walk(&ast, Wants::Value, many_names, &mut marks);
    if let ASTNodeType::Function { name, args } = &ast.node_type {
        // A plain formula shows one value of what it gives, and never spills.
        let gives_array = matches!(
            name.as_str(),
            "TRANSPOSE"
                | "MMULT"
                | "MINVERSE"
                | "FREQUENCY"
                | "LINEST"
                | "LOGEST"
                | "TREND"
                | "GROWTH"
                | "MODE.MULT"
                | "_xlfn.MODE.MULT"
                | "MUNIT"
                | "_xlfn.MUNIT"
        ) || args
            .iter()
            .any(|arg| matches!(arg.node_type, ASTNodeType::Array(_)))
            && args.iter().enumerate().all(|(at, arg)| {
                !matches!(arg.node_type, ASTNodeType::Array(_))
                    || matches!(
                        param_class(name, at),
                        Some(ParamClass::Value | ParamClass::Scalar)
                    )
            });
        if gives_array {
            if let Some(token) = &ast.source_token {
                marks.push(token.start);
            }
        }
    }
    if matches!(
        &ast.node_type,
        ASTNodeType::BinaryOp {
            op: BinaryOperator::Intersection,
            ..
        }
    ) {
        return Some(format!("=@({text})"));
    }
    if marks.is_empty() {
        return None;
    }
    marks.sort_unstable();
    marks.dedup();
    let mut out = String::with_capacity(source.len() + marks.len());
    let mut from = 0;
    for at in marks {
        // A position that isn't on a character boundary would mean the positions are wrong.
        out.push_str(source.get(from..at)?);
        out.push('@');
        from = at;
    }
    out.push_str(source.get(from..)?);
    Some(out)
}

/// Whether any word in the text is one of the names, outside strings.
pub(crate) fn mentions_name(text: &str, many_names: &dyn Fn(&str) -> bool) -> bool {
    let mut quote = None;
    let mut start = None;
    for (at, ch) in text.char_indices() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '\\' {
            start.get_or_insert(at);
            continue;
        }
        if let Some(begun) = start.take() {
            if ch != '(' && ch != '!' && many_names(&text[begun..at]) {
                return true;
            }
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        }
    }
    start.is_some_and(|begun| many_names(&text[begun..]))
}

#[cfg(test)]
mod tests {
    use super::mark;

    fn marked(text: &str) -> String {
        mark(text, &|name| name.eq_ignore_ascii_case("Block")).unwrap_or_else(|| text.to_string())
    }

    #[test]
    fn value_parameters_read_one_cell_and_reference_parameters_all() {
        for (text, expected) in [
            ("SUM(A1:A9)", "SUM(A1:A9)"),
            ("LEFT(A1:A9,1)", "=LEFT(@A1:A9,1)"),
            ("SUM(A1:A9*2)", "=SUM(@A1:A9*2)"),
            ("A:A&\"/\"&B:B", "=@A:A&\"/\"&@B:B"),
            ("SUMPRODUCT(A1:A9*2)", "SUMPRODUCT(A1:A9*2)"),
            ("SUMPRODUCT(LEN(A1:A9))", "SUMPRODUCT(LEN(A1:A9))"),
            (
                "SUMPRODUCT(IF(A1:A9>0,1,0))",
                "=SUMPRODUCT(IF(@A1:A9>0,1,0))",
            ),
            ("IF(A:A>1,B:B,C:C)", "=IF(@A:A>1,@B:B,@C:C)"),
            ("SUM(IF(A:A>1,B:B,C:C))", "=SUM(IF(@A:A>1,B:B,C:C))"),
            ("MATCH(A:A,B:B,0)", "=MATCH(@A:A,B:B,0)"),
            ("MATCH(1,(A:A=1)*B:B,0)", "=MATCH(1,(@A:A=1)*@B:B,0)"),
            ("INDEX(A1:A9*2,0)", "INDEX(A1:A9*2,0)"),
            ("VLOOKUP(A:A,B:C,2,0)", "=VLOOKUP(@A:A,B:C,2,0)"),
            (
                "LOOKUP(2,1/(A1:A9<>\"\"),B1:B9)",
                "LOOKUP(2,1/(A1:A9<>\"\"),B1:B9)",
            ),
            ("SUMIF(A:A,B:B,C:C)", "=SUMIF(A:A,@B:B,C:C)"),
            ("COUNTIFS(A:A,B:B,C:C,D:D)", "=COUNTIFS(A:A,@B:B,C:C,@D:D)"),
            ("SUM(Sheet2!A1:A3)", "SUM(Sheet2!A1:A3)"),
            ("ABS('My Sheet'!$A:$A)", "=ABS(@'My Sheet'!$A:$A)"),
            ("Block*2", "=@Block*2"),
            ("SUM(Block)", "SUM(Block)"),
            ("A1:A1+1", "A1:A1+1"),
            ("FILTER(A1:A9,A1:A9>1)", "FILTER(A1:A9,A1:A9>1)"),
            ("SUM(ROW(A1:A9))", "=SUM(@ROW(A1:A9))"),
            ("ROW(A1:A9)-1", "=@ROW(A1:A9)-1"),
            ("SUMPRODUCT(ROW(A1:A9)*2)", "SUMPRODUCT(ROW(A1:A9)*2)"),
            (
                "SUMPRODUCT(MAX((A1:A9=1)*ROW(A1:A9)))",
                "SUMPRODUCT(MAX((A1:A9=1)*ROW(A1:A9)))",
            ),
            ("ROW(A1)", "ROW(A1)"),
            ("LEN(INDIRECT(B1))", "=LEN(@INDIRECT(B1))"),
            ("SUM(INDIRECT(B1))", "SUM(INDIRECT(B1))"),
            ("OFFSET(A1,1,1)", "OFFSET(A1,1,1)"),
            ("OFFSET(A1,1,1,3)", "=@OFFSET(A1,1,1,3)"),
            ("SUM(OFFSET(A1,1,1,3))", "SUM(OFFSET(A1,1,1,3))"),
            ("\"a:b\"&A1", "\"a:b\"&A1"),
        ] {
            assert_eq!(marked(text), expected, "{text}");
        }
    }
}
