//! What a held item can cost in output, beyond its own bytes.
//!
//! The byte caps in [`super::input`] bound the output only if every held byte is charged for each place a writer can
//! put it. A string is written once in the text body, once more in the JSON `body`, and once as a raw copy in the
//! JSON `sections`, `error` and `hints`: at most 6 + 7 + 6 = 19 output bytes per held byte (a control character is
//! the worst case, `\u0001` in the text, `\\u0001` in the JSON `body`). Charging each string once covers that. The
//! writers also put some strings in more places, and those places are charged here:
//!
//! | String | Written again | Charge |
//! | --- | --- | --- |
//! | Field name | in the `truncated` table, once per cut cell of its column | its length per cut-capable cell |
//! | Section name | in the `truncated` table once per cut cell of the section when two or more sections cut, and in the `totalCount` table beside its header | its length per cut-capable cell, and twice per section |
//!
//! Only a string cell longer than [`CUT_CHARS`] chars is cut (the exports never change the response's cap; a test
//! scans for it). Everything else is written in a fixed number of places, so it is charged its bytes plus a fixed
//! allowance for the punctuation the writers add around it.

use super::value::JsToonValue;

/// The per-cell cap every JS-built response renders with: [`crate::response::DEFAULT_TRUNCATE_CELLS`]. No export
/// changes it.
pub(crate) const CUT_CHARS: usize = crate::response::DEFAULT_TRUNCATE_CELLS;

/// Longest fixed field name michi writes into a table it composes itself (`operation` in the partial-success table),
/// charged beside the section name for each cut cell of those tables. A test checks it against the tables.
pub(crate) const FIXED_FIELD_BYTES: usize = 9;

/// Bytes charged for each hint beyond its own: the quotes and separators it is written with, up to 6 bytes across the
/// three surfaces, and nothing else holds an empty hint's place.
pub(crate) const HINT_OVERHEAD: usize = 1;

/// Bytes charged for each recovery hint beyond its tool and reason: a row of the `recovery` table, its JSON object
/// (`{"tool":…,"params":{},"reason":…}`) and the escaped row inside the JSON `body`, about 50 bytes in all.
pub(crate) const RECOVERY_OVERHEAD: usize = 4;

/// Whether rendering cuts `s`, which writes the table and section names again in the `truncated` block. Byte length
/// is at least the char count, so a short string is not cut without scanning it.
pub(crate) fn is_cut(s: &str) -> bool {
    s.len() > CUT_CHARS && s.chars().count() > CUT_CHARS
}

/// How many of `cells` rendering cuts.
pub(crate) fn cut_count<'a>(cells: impl IntoIterator<Item = &'a str>) -> usize {
    cells.into_iter().filter(|s| is_cut(s)).count()
}

/// The bytes the `truncated` table can write again for a table: for every cell that is cut, the column's field name
/// and the section's name (`section_len`, 0 where the table cannot share the `truncated` block).
pub(crate) fn table_repeats(rows: &[Vec<JsToonValue>], fields: &[String], section_len: usize) -> usize {
    let mut total = 0usize;
    for row in rows {
        for (cell, field) in row.iter().zip(fields) {
            if cell.str_val.as_deref().is_some_and(is_cut) {
                total = total.saturating_add(field.len()).saturating_add(section_len);
            }
        }
    }
    total
}

/// What the `truncated` block can write again for `cut` cells of a table the response composes itself, whose field
/// names are fixed: the section's name and a fixed field name for each.
pub(crate) fn fixed_table_repeats(cut: usize, section_len: usize) -> usize {
    cut.saturating_mul(section_len.saturating_add(FIXED_FIELD_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(s: &str) -> JsToonValue {
        JsToonValue { r#type: "str".into(), str_val: Some(s.into()), ..JsToonValue::default() }
    }

    #[test]
    fn a_cell_is_cut_past_the_cap_in_chars_not_bytes() {
        assert!(!is_cut(&"a".repeat(CUT_CHARS)));
        assert!(is_cut(&"a".repeat(CUT_CHARS + 1)));
        // 200 two-byte chars are 400 bytes and not cut.
        assert!(!is_cut(&"é".repeat(CUT_CHARS)));
        assert!(is_cut(&"é".repeat(CUT_CHARS + 1)));
    }

    #[test]
    fn a_table_is_charged_its_field_and_section_names_once_per_cut_cell() {
        let rows = vec![
            vec![cell(&"a".repeat(201)), cell("short")],
            vec![cell(&"a".repeat(300)), cell(&"b".repeat(201))],
            vec![JsToonValue { r#type: "null".into(), ..JsToonValue::default() }, cell("x")],
        ];
        let fields = ["f".repeat(10), "g".repeat(100)];
        // Three cut cells: two under `f` (10) and one under `g` (100), each with a section name of 7.
        assert_eq!(table_repeats(&rows, &fields, 7), 2 * (10 + 7) + (100 + 7));
        assert_eq!(table_repeats(&rows, &fields, 0), 2 * 10 + 100);
        assert_eq!(table_repeats(&[], &fields, 7), 0);
    }

    #[test]
    fn the_fixed_field_allowance_covers_every_table_michi_composes() {
        let longest = crate::idempotency::FIELDS
            .iter()
            .chain(&crate::pipeline::FIELDS)
            .chain(&["key", "value", "health", "reason"])
            .map(|f| f.len())
            .max();
        assert_eq!(longest, Some(FIXED_FIELD_BYTES));
    }
}
