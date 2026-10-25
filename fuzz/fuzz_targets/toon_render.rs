//! `render_toon` never panics, and every `Ok` document is canonical TOON 4.4 whose line count, header and cells
//! match what was asked for. The strict decoder is the same test-only oracle the integration tests use.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::toon::{render_toon, ToonOptions, Value};

#[path = "../../crates/michi-core/tests/support/toon_parser.rs"]
mod toon_parser;

#[derive(Arbitrary, Debug)]
enum Cell {
    Str(String),
    Int(i64),
    UInt(u64),
    /// Raw bits, so NaN payloads, infinities, subnormals and -0.0 all show up.
    Float(u64),
    Bool(bool),
    Null,
}

impl Cell {
    fn value(&self) -> Value {
        match self {
            Cell::Str(s) => Value::Str(s.as_str().into()),
            Cell::Int(i) => Value::Int(*i),
            Cell::UInt(u) => Value::UInt(*u),
            Cell::Float(bits) => Value::Float(f64::from_bits(*bits)),
            Cell::Bool(b) => Value::Bool(*b),
            Cell::Null => Value::Null,
        }
    }
}

#[derive(Arbitrary, Debug)]
struct Input {
    type_name: String,
    fields: Vec<String>,
    /// Cells are dealt into rows of `fields.len()`, so most inputs are well-formed.
    cells: Vec<Cell>,
    rows: u8,
    /// Drop one cell from the last row, to reach the row-length error.
    ragged: bool,
    total_count: Option<u32>,
    hints: Vec<String>,
    max_cell_len: Option<u8>,
    truncation_hint: Option<String>,
}

fuzz_target!(|input: Input| {
    let width = input.fields.len();
    if input.cells.is_empty() && input.rows > 0 && width > 0 {
        return;
    }
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut next = 0usize;
    for _ in 0..input.rows {
        let mut row = Vec::with_capacity(width);
        for _ in 0..width {
            row.push(input.cells[next % input.cells.len()].value());
            next += 1;
        }
        rows.push(row);
    }
    if input.ragged {
        if let Some(last) = rows.last_mut() {
            last.pop();
        }
    }
    let nrows = rows.len();
    let mut opts = ToonOptions::new(input.type_name.clone(), input.fields.clone(), rows.clone())
        .total_count(input.total_count.map(|n| n as usize))
        .hints(input.hints.clone());
    if let Some(n) = input.max_cell_len {
        opts = opts.max_cell_len(n as usize);
    }
    if let Some(h) = &input.truncation_hint {
        opts = opts.truncation_hint(h.clone());
    }

    let Ok(out) = render_toon(&opts) else { return };

    // The cut rule, written out here independently of the library: a string cell of more than `cap` chars keeps its
    // first `cap - 1` chars and ends in `…` (a cap of 0 gives the empty string), and the cut is reported in the
    // `truncated` block as (row, field, chars removed).
    let cap = input.max_cell_len.map_or(200, usize::from);
    let kept = cap.saturating_sub(1);
    let mut cuts: Vec<(usize, usize, String, usize)> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            if let Value::Str(s) = value {
                let chars = s.chars().count();
                if chars > cap {
                    let shown: String =
                        if cap == 0 { String::new() } else { s.chars().take(kept).chain(['…']).collect() };
                    cuts.push((r, c, shown, chars - kept));
                }
            }
        }
    }

    // With a non-empty truncation hint and at least one cut, one more line joins the caller's `help` hints.
    let mut expected_hints = input.hints.clone();
    if let Some(hint) = input.truncation_hint.as_deref().filter(|h| !h.is_empty() && !cuts.is_empty()) {
        expected_hints.push(format!("Cut cells are listed in the truncated block; use {hint} for the full value"));
    }

    // Escaping: no raw CR, and the line count is exactly what the header declares, so no cell or hint made a line.
    assert!(!out.contains('\r'), "raw CR in {out:?}");
    let truncated_lines = if cuts.is_empty() { 0 } else { 1 + cuts.len() };
    let expected_lines = 1
        + nrows
        + usize::from(input.total_count.is_some())
        + usize::from(!expected_hints.is_empty())
        + truncated_lines;
    assert_eq!(out.matches('\n').count() + 1, expected_lines, "line count in {out:?}");

    // Canonical TOON 4.4: the strict decoder rejects anything an encoder must not emit.
    let parsed = toon_parser::parse(&out).unwrap_or_else(|e| panic!("not conformant TOON: {e}\n{out:?}"));
    assert_eq!(parsed.type_name, input.type_name);
    assert_eq!(parsed.rows.len(), nrows);
    if nrows > 0 {
        assert_eq!(parsed.fields, input.fields);
    }
    assert_eq!(parsed.total_count, input.total_count.map(|n| n as usize));
    assert_eq!(parsed.hints, expected_hints);
    let reported: Vec<(usize, String, usize)> =
        cuts.iter().map(|(r, c, _, removed)| (*r, input.fields[*c].clone(), *removed)).collect();
    assert_eq!(parsed.truncated, reported, "truncated block in {out:?}");

    // Every cell decodes to its value; a cut string cell to exactly its kept chars plus `…`.
    for (r, (parsed_row, row)) in parsed.rows.iter().zip(&rows).enumerate() {
        assert_eq!(parsed_row.len(), width);
        for (c, (got, value)) in parsed_row.iter().zip(row).enumerate() {
            match value {
                Value::Str(s) => {
                    let want = cuts
                        .iter()
                        .find(|(cr, cc, ..)| (*cr, *cc) == (r, c))
                        .map_or_else(|| s.to_string(), |cut| cut.2.clone());
                    assert_eq!(got, &toon_parser::Prim::Str(want), "string cell {s:?}");
                }
                // `value_to_prim` spells floats with `to_string`, which is not canonical TOON for tiny or huge values;
                // compare those by value instead: finite floats decode to the same number, the rest to null.
                Value::Float(f) if f.is_finite() && *f != 0.0 => {
                    let toon_parser::Prim::Num(text) = got else { panic!("float {f:?} decoded as {got:?}") };
                    let back: f64 = text.parse().expect("canonical number parses");
                    assert_eq!(back.to_bits(), f.to_bits(), "float {f:?} rendered as {text}");
                }
                other => assert_eq!(got, &toon_parser::value_to_prim(other), "cell {other:?}"),
            }
        }
    }
});
