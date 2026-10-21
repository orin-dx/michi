# TOON Format

TOON (Token-Oriented Object Notation) is the agent-facing list format. It front-loads structure — type, count, field names — so the model has full context before reading a single row, then encodes rows as compact comma-separated values. Field names appear once, in the header, not once per row.

This is the canonical implementation of **AXI Principle 1 (Token-Efficient Output)**. Every token in a response permanently consumes context-window budget, and that cost compounds across turns. TOON skips the braces, quotes, and repeated keys JSON spends on structure the model can already infer positionally — roughly 40% fewer tokens than equivalent JSON for list data.

**Conformance:** `toon-spec: 4.4`. Every TOON document michi emits is valid encoder output under the [TOON specification](https://github.com/toon-format/spec/blob/main/SPEC.md), Version 4.4 (Working Draft, 2026-10-08), with the default options (comma delimiter, 2-space indent). `crates/michi-core/tests/toon_spec.rs` covers each applicable encoder rule by section, and `tests/toon_spec_fixtures.rs` runs the official encode fixtures (vendored in `tests/fixtures/toon-spec/`) that fall inside michi's subset.

## When to use it

JSON repeats field names on every item. Markdown key-value repeats them too — fine for a single item or a handful, expensive at scale. TOON trades per-item repetition for a one-time header, so the savings grow with N. Use TOON for 5+ uniform-schema rows; use `kv::render_kv()` for single items or small mixed-type metadata.

## Grammar

michi emits this subset of TOON 4.4. Lines are separated by LF; the document has no trailing newline, no trailing spaces, no blank lines and no byte-order mark (§12).

```
document     ::= list (LF totalcount)? (LF help)?      (a root object, §5)
list         ::= key ": []"                            (no rows, §9.1)
               | key "[" count "]:" (LF "  -")+        (rows with zero fields: empty objects, §9.4/§10)
               | key "[" count "]{" fields "}:" (LF row)+   (tabular, §9.3)
fields       ::= key ("," key)*                        (at least one, all distinct, §6)
row          ::= "  " cell ("," cell)*                 (one cell per field)
cell         ::= string | number | "true" | "false" | "null"
totalcount   ::= "totalCount: " digits                 (§8)
help         ::= "help[" count "]: " string ("," string)*   (inline primitive array, §9.1)
count        ::= "0" | [1-9][0-9]*                     (equals the number of rows, items or hints)

key          ::= unquoted-key | quoted                 (§7.3)
unquoted-key ::= [A-Za-z_][A-Za-z0-9_.]*
string       ::= unquoted-string | quoted              (§7.2)
quoted       ::= '"' (char | escape)* '"'              (§7.1)
escape       ::= '\\' | '\"' | '\n' | '\r' | '\t' | '\u00' hex hex   (lowercase hex)
number       ::= canonical decimal, or exponent form outside 1e-6 <= |n| < 1e21   (§2)
```

The type name is the list's key; `totalCount` and `help` are the document's other keys, so they are reserved and rejected as type names (`ToonError::InvalidTypeName`). A repeated field name is rejected too (`ToonError::InvalidFieldName`). Any other name is valid: names outside `^[A-Za-z_][A-Za-z0-9_.]*$` — including the empty name, non-ASCII names and names with spaces or punctuation — are quoted.

## Examples

**List response:**

```
issues[3]{number,title,state}:
  42,Fix login redirect,open
  43,Add dark mode,open
  44,"Update deps, bump major",closed
totalCount: 47
help[2]: Call get_issue with number=<number> for full detail,Call list_issues with state=open to filter
```

**Truncated field value** (`.max_cell_len(55).truncation_hint("full=true")`, descriptions of 170 and 225 chars):

```
components[2]{name,description,tokens}:
  Button,Primary action element (148 more chars — use full=true),12
  Icon,Scalable vector glyphs (203 more chars — use full=true),8
totalCount: 84
help[1]: Call get_component with name=<name> and full=true for complete description
```

**Empty state:**

```
issues: []
totalCount: 0
help[1]: Try list_issues with a broader filter
```

**Quoted keys, nulls and numbers:**

```
"pull-requests"[2]{id,"head ref",merged_at,score}:
  18446744073709551615,"feature/x:y",null,0.000001
  7,main,"2026-10-21T09:00:00Z",1e+21
```

**Single recovery hint (no list):**

```
recovery[1]:
  create_item: suggestedParams: { project: PROJ, type: Task }
```

A dedicated `recovery[N]:` block, not folded into `help[]` — see [03-rust-api.md](03-rust-api.md)'s `recovery` section for why. Param values render unquoted in plain-text form; `type` above is `KvValue::Text("Task")`, not a JSON string literal. The recovery block is michi's own line format, not TOON — see [06-decisions.md](06-decisions.md) ("TOON output conforms to TOON 4.4") for which blocks are and are not TOON.

## Strings

A string cell, hint or key is written bare unless TOON 4.4 §7.2 requires quotes. It is quoted when it:

- is empty (`""`),
- has leading or trailing whitespace (space or tab),
- is exactly `true`, `false` or `null`,
- looks numeric: `/^[+-]?[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$/i` — `42`, `-3.14`, `05`, `+1`, `1e-6`,
- contains `:`, `"`, `\`, `[`, `]`, `{`, `}`, a comma, or any control character U+0000–U+001F,
- starts with `-` or `#`.

Everything else — Unicode, emoji, interior spaces, `|`, `<`, `=` — stays bare. Inside quotes, `\` and `"` are escaped as `\\` and `\"`, LF, CR and tab as `\n`, `\r` and `\t`, other control characters as `\u00xx` with lowercase hex; nothing else is escaped. A cell or hint therefore never spans lines and nothing is dropped. A truncated cell is quoted by the same rules after truncation (the suffix needs none unless the caller's hint contains a character that does).

## Truncation

A string cell longer than `max_cell_len` chars (default 200) is cut, and its text says so:

- The cell keeps as many leading chars as fit and ends with `" ({removed} more chars)"`, where `removed` is the number of chars cut, not the original length. Kept chars plus `removed` equal the original char count, and the whole cell, suffix included, is at most `max_cell_len` chars.
- The `— use {hint}` clause, as in `" (148 more chars — use full=true)"`, appears only when the caller sets `ToonOptions::truncation_hint` (`AgentResponse::truncation_hint`, NAPI `truncationHint`). michi does not know the calling tool's parameters, so it never invents one; an empty hint is the same as none.
- When `max_cell_len` cannot hold the suffix plus one kept char, the cell is its first `max_cell_len - 1` chars followed by `…` (just `…` at 1, the empty string `""` at 0), never a fragment of the suffix.
- The cap counts chars **before** escaping and quoting: a truncated cell is then quoted and escaped by the rules above, so a cell holding `"`, `\`, a line break or a comma is written longer than `max_cell_len`.
- Numbers, booleans and `null` are never truncated.

## Numbers, booleans and null

- Integers render with their exact digits, including `u64`/`usize` values above `i64::MAX` (`Value::UInt`, also produced by serde `list()`). Numeric cells are never truncated.
- Floats use the shortest digits that round-trip, without exponent for `1e-6 <= |n| < 1e21` and without trailing fractional zeros (`1.0` → `1`, `1e6` → `1000000`); outside that range they use a lowercase, signed exponent (`1e+21`, `2.5e-7`). `-0` renders as `0`; NaN and ±Infinity render as `null`.
- Booleans: `true` / `false`. `Value::Null` and a missing serde field render as `null`, distinct from the empty string `""`.

## Out of scope

michi emits flat lists only, so these parts of TOON 4.4 never appear in its output: nested objects (§8), nested field groups and keyed tabular objects (§9.3, §9.5), arrays of arrays and mixed lists (§9.2, §9.4 beyond empty objects), the tab and pipe delimiters (§11), other indent sizes (§12), root primitives and root arrays (§5). serde `list()` normalizes a nested array or object to a string holding its compact JSON text (§3 host-type normalization) instead of emitting nested TOON.
