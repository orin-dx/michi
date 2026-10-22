# TOON Format

TOON (Token-Oriented Object Notation) is the agent-facing list format. It front-loads structure — type, count, field names — so the model has full context before reading a single row, then encodes rows as compact comma-separated values. Field names appear once, in the header, not once per row.

This is the canonical implementation of **AXI Principle 1 (Token-Efficient Output)**. Every token in a response permanently consumes context-window budget, and that cost compounds across turns. TOON skips the braces, quotes, and repeated keys JSON spends on structure the model can already infer positionally — roughly 40% fewer tokens than equivalent JSON for list data.

**Conformance:** `toon-spec: 4.4`. Every agent-facing block michi emits — lists, key-value, errors, status, recovery, partial success, already-done, hints — is valid encoder output under the [TOON specification](https://github.com/toon-format/spec/blob/main/SPEC.md), Version 4.4 (Working Draft, 2026-10-08), with the default options (comma delimiter, 2-space indent). `crates/michi-core/tests/toon_spec.rs` covers each applicable encoder rule by section, and `tests/toon_spec_fixtures.rs` runs the official encode fixtures (vendored in `tests/fixtures/toon-spec/`) that fall inside michi's subset.

## When to use it

JSON repeats field names on every item. Markdown key-value repeats them too — fine for a single item or a handful, expensive at scale. TOON trades per-item repetition for a one-time header, so the savings grow with N. Use TOON for 5+ uniform-schema rows; use `kv::render_kv()` for single items or small mixed-type metadata.

## Grammar

michi emits this subset of TOON 4.4. Lines are separated by LF; the document has no trailing newline, no trailing spaces, no blank lines and no byte-order mark (§12).

```
document     ::= list (LF totalcount)? (LF help)? (LF recovery)?   (a list document: a root object, §5)
               | entry (LF entry)*                     (any other block: a root object, unique keys, §14.3)
entry        ::= key ": " cell | list | inline | recovery   (primitive field §8, arrays §9)
inline       ::= key "[" count "]: " cell ("," cell)*   (inline primitive array, §9.1)
recovery     ::= "recovery[" count "]{tool,params,reason}:" (LF row)+   (tabular, §9.3)
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

The type name is the list's key; `totalCount`, `help` and `recovery` are the keys michi composes next to it (`AgentResponse` appends `recovery`), so they are reserved and rejected as type names (`ToonError::InvalidTypeName`). A repeated field name is rejected too (`ToonError::InvalidFieldName`). Any other name is valid: names outside `^[A-Za-z_][A-Za-z0-9_.]*$` — including the empty name, non-ASCII names and names with spaces or punctuation — are quoted.

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
recovery[1]{tool,params,reason}:
  create_item,"{\"project\":\"PROJ\",\"type\":\"Task\"}",project is required
```

A dedicated `recovery[N]` table, not folded into `help[]`: hints are strings, recovery hints are records. See [Agent-facing blocks](#agent-facing-blocks) for its columns.

## Agent-facing blocks

Every block michi renders for an agent — not only lists — is a TOON 4.4 root object with the default options, so an agent parses one format. Each renderer returns a finished document with no trailing newline; `append_hints` and `append_recovery` join a block onto a document with exactly one LF (none when the buffer is empty or already ends in one). Composition keeps keys unique: a list's type name and a KV key may not be `totalCount`, `help` or `recovery`, and KV keys may not repeat (`ToonError::InvalidTypeName`, `KvError`). Strings and keys are quoted per [Strings](#strings); numbers follow [Numbers, booleans and null](#numbers-booleans-and-null).

**Key-value (`kv::render_kv`, `AgentResponse` KV path)** — one `key: value` field per item (§8), exactly one space after the colon (no column alignment), then `totalCount` and `help`:

```
id: abc-123
title: "Fix login: redirect loop"
score: 0.25
wait: 1.5s
assignee: null
totalCount: 1
help[1]: Call update_issue with id=<id>
```

`KvValue::Text` is a string; `Int` and `Bool` are literals; `Float(f, decimals)` is `f` rounded to `decimals` places, written in canonical form (`Float(1.5, 3)` → `1.5`, NaN → `null`); `Duration` is the string `1.5s`; `Missing` is `null`.

**Recovery hints (`recovery::render_recovery`, appended by `AgentResponse`, `DomainError` and `PartialSuccess`)** — a tabular array (§9.3) with three primitive columns. `params` is the string holding the parameters' compact JSON object, exactly the `params` object of the JSON companions (`"{}"` when empty); `reason` is `null` when absent:

```
recovery[2]{tool,params,reason}:
  retry,"{\"attempt\":2}",transient network error
  reload_cache,"{}",null
```

**Errors (`DomainError::render`, `Error::render`)**:

```
error: not_found
message: "Issue #9999 not found: check the number"
exit_code: 1
help[1]: Call list_issues to see available numbers
recovery[1]{tool,params,reason}:
  list_issues,"{\"state\":\"open\"}",null
```

`Error::InvalidInput(m)` and `Error::NotFound(m)` render the same shape with `error: invalid_input` / `error: not_found` and `message: m`. `AgentResponse`'s validation failures are `error: toon_validation_failed` / `error: kv_validation_failed` plus a quoted `message`.

**Status (`StatusResponse::render`)** — item keys are cells of a table, not root keys, so any key is valid; `health` is `ok`, `degraded`, `error`, or `null` for no signal, and `reason` is `null` unless degraded or error:

```
tool: my-tool
description: Semantic code search
items[3]{key,value,health,reason}:
  index,ready,ok,null
  cache,warm,degraded,near limit
  workers,4,null,null
help[1]: Call get_status for detail
```

With no items the table is `items: []`.

**Already done (`resilience::render_already_done`)**:

```
operation: delete_item
status: already_done
summary: item 42 was deleted in run abc
help[1]: Call get_item with id=42 to confirm
```

**Partial success (`PartialSuccess::render`)** — the summary is a quoted string, `completed`/`skipped` inline arrays (§9.1), `failed` a table; empty categories are omitted:

```
partial_success: "2 completed, 1 failed, 1 skipped"
completed[2]: create_issue,add_label
failed[1]{operation,reason}:
  assign_user,User 'ghost' not found
skipped[1]: notify_team
recovery[1]{tool,params,reason}:
  assign_user,"{\"user\":\"alice\"}",null
```

`crates/michi-core/tests/toon_blocks.rs` pins each form and checks every block with the strict validator in `tests/support/toon_parser.rs`. Why each form was chosen: [06-decisions.md](06-decisions.md) ("Every agent-facing block is TOON 4.4").

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

## Size limits

`ToonOptions::validate()` rejects a list whose shape would make rendering unreasonably large, before it looks at any row. The limits are public constants in `michi_core::toon`:

| Limit | Constant | Error |
| --- | --- | --- |
| At most 1,000 header fields | `MAX_FIELDS` | `ToonError::TooManyFields { count, max }` |
| At most 1,000,000 cells (`rows × fields`) | `MAX_CELLS` | `ToonError::TooManyCells { rows, fields, max }` |
| A `truncation_hint` of at most 256 chars | `MAX_TRUNCATION_HINT_CHARS` | `ToonError::TruncationHintTooLong { len, max }` |

- Checks run in a fixed order and the first failure wins: reserved type name, field count, repeated field, cell count, truncation hint length, row length.
- Every check is linear: repeated field names are found with a set, so the largest accepted header costs 1,000 lookups instead of ~500,000 comparisons.
- serde `list()` checks the field and cell limits against the union of the items' keys before it builds any row, so a sparse item set (each item with a few distinct keys) cannot expand into a huge table of `null` cells.
- `AgentResponse` reports a violation like any other validation failure: an `error: toon_validation_failed` body with `isError` set on the text, JSON and MCP surfaces.
- The truncation hint is copied into every truncated cell, which is why its length is capped. The renderer also measures it once per document, not once per cell.

## Numbers, booleans and null

- Integers render with their exact digits, including `u64`/`usize` values above `i64::MAX` (`Value::UInt`, also produced by serde `list()`). Numeric cells are never truncated.
- Floats use the shortest digits that round-trip, without exponent for `1e-6 <= |n| < 1e21` and without trailing fractional zeros (`1.0` → `1`, `1e6` → `1000000`); outside that range they use a lowercase, signed exponent (`1e+21`, `2.5e-7`). `-0` renders as `0`; NaN and ±Infinity render as `null`.
- Booleans: `true` / `false`. `Value::Null` and a missing serde field render as `null`, distinct from the empty string `""`.

## Out of scope

michi emits flat lists only, so these parts of TOON 4.4 never appear in its output: nested objects (§8), nested field groups and keyed tabular objects (§9.3, §9.5), arrays of arrays and mixed lists (§9.2, §9.4 beyond empty objects), the tab and pipe delimiters (§11), other indent sizes (§12), root primitives and root arrays (§5). serde `list()` normalizes a nested array or object to a string holding its compact JSON text (§3 host-type normalization) instead of emitting nested TOON.

## External interoperability gate

The official TOON 4.4 encode fixtures (`tests/toon_spec_fixtures.rs`) are external evidence that michi's encoder conforms to the specification for the subset it emits. They do not show that independent decoders or other TOON implementations accept michi's output; track those comparisons and any compatibility claims in the [TOON interoperability research note](../research/toon-interoperability.md).
