//! The one writer for michi's own TOON 4.4 tabular blocks (§9.3): `key[N]{c1,c2}:` and one row per line at depth 1.
//! Every block michi composes (`truncated`, `totalCount[N]`, the budget roll-up, the partial-success tables,
//! `recovery`) goes through it, so header syntax and cell quoting live in one place.

use super::escape::{escape_key, escape_value};

/// One cell of a michi-written table row.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cell<'a> {
    /// A string, quoted and escaped per §7.2 when it needs it.
    Str(&'a str),
    /// An unsigned integer, written as is.
    Num(u64),
    /// The `null` literal.
    Null,
    /// `true` or `false`.
    Bool(bool),
}

impl<'a> Cell<'a> {
    /// A string cell, or `null` when absent.
    pub(crate) fn opt_str(s: Option<&'a str>) -> Self {
        s.map_or(Self::Null, Self::Str)
    }

    /// A count cell. `usize` fits `u64` on every platform Rust supports.
    pub(crate) fn count(n: usize) -> Self {
        Self::Num(n as u64)
    }

    /// A count cell, or `null` when absent.
    pub(crate) fn opt_count(n: Option<usize>) -> Self {
        n.map_or(Self::Null, Self::count)
    }
}

/// Append `key[len]{c1,c2}:` on a line of its own (a LF first unless `out` is empty or already ends in one). The key
/// and the column names are encoded as TOON keys (§7.3).
pub(crate) fn push_header<I>(out: &mut String, key: &str, len: usize, columns: I)
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    use std::fmt::Write as _;
    crate::hints::start_block(out);
    out.push_str(&escape_key(key));
    let _ = write!(out, "[{len}]{{");
    for (i, column) in columns.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&escape_key(column.as_ref()));
    }
    out.push_str("}:");
}

/// Append one row: a LF, two spaces of indent, then the cells separated by commas.
pub(crate) fn push_row(out: &mut String, cells: &[Cell<'_>]) {
    use std::fmt::Write as _;
    out.push_str("\n  ");
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match cell {
            Cell::Str(s) => out.push_str(&escape_value(s)),
            Cell::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Cell::Null => out.push_str("null"),
            Cell::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        }
    }
}

/// One cut cell, as the `truncated` block reports it.
#[derive(Debug, Clone)]
pub(crate) struct Cut<'a> {
    /// The block the cell belongs to; written only in the multi-block form.
    pub(crate) section: &'a str,
    /// Zero-based row of the cell.
    pub(crate) row: usize,
    /// The cell's field name.
    pub(crate) field: &'a str,
    /// Chars cut off the cell.
    pub(crate) removed: usize,
}

/// Append the `truncated` block for `cuts`, nothing when there are none. One block uses `{row,field,removed}`; with
/// `per_section` the rows lead with a `section` column, so several blocks can share the one `truncated` root key.
pub(crate) fn push_truncated(out: &mut String, cuts: &[Cut<'_>], per_section: bool) {
    if cuts.is_empty() {
        return;
    }
    let columns: &[&str] =
        if per_section { &["section", "row", "field", "removed"] } else { &["row", "field", "removed"] };
    push_header(out, "truncated", cuts.len(), columns);
    for c in cuts {
        let tail = [Cell::count(c.row), Cell::Str(c.field), Cell::count(c.removed)];
        if per_section {
            push_row(out, &[Cell::Str(c.section), tail[0], tail[1], tail[2]]);
        } else {
            push_row(out, &tail);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_starts_its_own_line_and_quotes_keys_that_need_it() {
        let mut out = String::from("x: 1");
        push_header(&mut out, "t", 2, &["a", "b c"]);
        assert_eq!(out, "x: 1\nt[2]{a,\"b c\"}:");
        let mut fresh = String::new();
        push_header(&mut fresh, "t", 0, &["a"]);
        assert_eq!(fresh, "t[0]{a}:");
    }

    #[test]
    fn rows_quote_strings_and_write_literals() {
        let mut out = String::new();
        push_row(&mut out, &[Cell::Str("a,b"), Cell::Num(3), Cell::Null, Cell::Bool(true), Cell::Str("null")]);
        assert_eq!(out, "\n  \"a,b\",3,null,true,\"null\"");
    }

    #[test]
    fn truncated_block_has_a_section_column_only_when_shared() {
        let cuts = [Cut { section: "s,1", row: 0, field: "c", removed: 31 }];
        let mut one = String::new();
        push_truncated(&mut one, &cuts, false);
        assert_eq!(one, "truncated[1]{row,field,removed}:\n  0,c,31");
        let mut shared = String::new();
        push_truncated(&mut shared, &cuts, true);
        assert_eq!(shared, "truncated[1]{section,row,field,removed}:\n  \"s,1\",0,c,31");
        let mut none = String::new();
        push_truncated(&mut none, &[], true);
        assert_eq!(none, "");
    }
}
