# TOON Format

TOON (Token-Optimized Object Notation) is the agent-facing list format. It front-loads structure — type, count, field names — so the model has full context before reading a single row, then encodes rows as compact comma-separated values. Field names appear once, in the header, not once per row.

This is the canonical implementation of **AXI Principle 1 (Token-Efficient Output)**. Every token in a response permanently consumes context-window budget, and that cost compounds across turns. TOON skips the braces, quotes, and repeated keys JSON spends on structure the model can already infer positionally — roughly 40% fewer tokens than equivalent JSON for list data.

## When to use it

JSON repeats field names on every item. Markdown key-value repeats them too — fine for a single item or a handful, expensive at scale. TOON trades per-item repetition for a one-time header, so the savings grow with N. Use TOON for 5+ uniform-schema rows; use `kv::render_kv()` for single items or small mixed-type metadata.

## Grammar

```
document     ::= type_header NEWLINE row+ totalcount? help_block?
type_header  ::= type_name "[" count "]" "{" field_list "}" ":"
type_name    ::= [a-z_][a-z0-9_]*                 (snake_case)
count        ::= [0-9]+                            (items in this response)
field_list   ::= field_name ("," field_name)*
field_name   ::= [a-z_][a-z0-9_]*
row          ::= "  " value ("," value)* NEWLINE   (2-space indent)
value        ::= scalar | quoted
scalar       ::= [^,\n"]*
quoted       ::= '"' ( [^"\\] | "\\" . )* '"'     (for values with commas)
totalcount   ::= "totalCount: " [0-9]+ NEWLINE     (total available, may exceed count)
help_block   ::= "help[" [0-9]+ "]:" NEWLINE hint+
hint         ::= "  " [^\n]+ NEWLINE
```

## Examples

**List response:**

```
issues[3]{number,title,state}:
  42,Fix login redirect,open
  43,Add dark mode,open
  44,"Update deps, bump major",closed
totalCount: 47
help[2]:
  Call get_issue with number=<number> for full detail
  Call list_issues with state=open to filter
```

**Truncated field value:**

```
components[2]{name,description,tokens}:
  Button,"Primary action element (148 chars truncated — use full=true)",12
  Icon,"Scalable vector icon (203 chars truncated — use full=true)",8
totalCount: 84
help[1]:
  Call get_component with name=<name> and full=true for complete description
```

**Empty state:**

```
issues[0]{}:
totalCount: 0
help[1]:
  Try list_issues with a broader filter
```

**Single recovery hint (no list):**

```
recovery[1]:
  create_item: suggestedParams: { project: PROJ, type: Task }
```

A dedicated `recovery[N]:` block, not folded into `help[]` — see [03-rust-api.md](03-rust-api.md)'s `recovery` section for why. Param values render unquoted in plain-text form; `type` above is `KvValue::Text("Task")`, not a JSON string literal.

## Escaping

- Values containing commas **must** be quoted
- Values containing double quotes use a backslash escape: `\"`
- Embedded newlines and carriage returns are silently stripped, not escaped — TOON has no multi-line cell format
- Null/absent values render as an empty scalar: `val1,,val3`
- Booleans: `true` / `false`
- Numbers: unquoted

## External interoperability gate

The grammar above defines Michi's dialect. Internal parser tests do not establish interoperability. Track external comparisons and compatibility claims in the [TOON interoperability research note](../research/toon-interoperability.md).
