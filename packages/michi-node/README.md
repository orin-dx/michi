# michi

TypeScript/JavaScript bindings for [`michi`](https://github.com/orin-dx/michi) — AXI response primitives for agent-ergonomic tools. Native binary via NAPI-RS: no WASM, no JS reimplementation of the rendering logic.

> [!IMPORTANT]\
> Not yet published to npm. See the [main repo](https://github.com/orin-dx/michi) for building from source in the meantime.

## Install

```bash
pnpm add @orin-dx/michi
```

## Quick start

```typescript
import { renderToon } from "@orin-dx/michi";

const out = renderToon({
  typeName: "issues",
  fields: ["number", "title", "state"],
  rows: [
    [
      { type: "int", intVal: 51815 },
      { type: "str", strVal: "[Bug]: Telegram plugin" },
      { type: "str", strVal: "open" },
    ],
    [
      { type: "int", intVal: 51812 },
      { type: "str", strVal: "dark mode request" },
      { type: "str", strVal: "open" },
    ],
  ],
  totalCount: 8771,
  hints: ["Run `gh-axi issue view <number>` to view an issue"],
});

process.stdout.write(out);
```

```text
issues[2]{number,title,state}:
  51815,"[Bug]: Telegram plugin",open
  51812,dark mode request,open
totalCount: 8771
help[1]: Run `gh-axi issue view <number>` to view an issue
```

## API

### Free functions

| Function | Signature |
| --- | --- |
| `renderToon` | `(opts: JsToonOptions) => string` — TOON list rendering |
| `emptyState` | `(typeName: string) => string` — definitive empty-state block |
| `renderHints` | `(hints: string[]) => string` — `help[N]:` block |
| `appendHints` | `(body: string, hints: string[]) => string` — append `help[N]:` to an existing body |
| `renderRecovery` | `(hints: JsRecoveryHint[]) => string` — `recovery[N]:` block |
| `truncate` | `(content: string, maxChars: number, hint: string) => string` — `truncate("a".repeat(200), 50, "full=true")` gives 17 × `a` + `" (183 more chars — use full=true)"`; an empty `hint` drops the `— use …` clause |

### `AgentResponse` class

The primary integration point — a builder that composes TOON/KV rendering, hints, and recovery into one response.

Setters return `void`, not `this`, so call them as sequential statements rather than a chain. (Chaining isn't supported across the NAPI boundary — see `docs/spec/04-mcp-and-napi.md` in the main repo for why.)

| Method | Signature |
| --- | --- |
| `constructor` | `(typeName: string)` |
| `.items` | `(rows: JsToonValue[][], fields: string[]) => void` |
| `.totalCount` | `(n: number) => void` |
| `.kvItems` | `(items: JsKvItem[]) => void` |
| `.hint` | `(hint: string) => void` |
| `.recoveryHint` | `(tool: string, reason?: string) => void` |
| `.truncationHint` | `(hint: string) => void` — the caller's instruction appended to truncated TOON cells as `— use {hint}` (same as `JsToonOptions.truncationHint`); without it the suffix only says how many chars were cut |
| `.asError` | `() => void` |
| `.renderToon` | `() => string` — slot-specific, reads only `items`/`fields` |
| `.renderKv` | `() => string` — slot-specific, reads only `kvItems` |
| `.renderJson` | `() => string` — compact JSON: `{"body":...,"hints":[...],"recovery":[...],"isError":bool}` |
| `.renderHintsOnly` | `() => string` — just the `help[N]:` block |
| `.humanContent` | `(text: string) => void` — attach a `user`-audience companion block for `toCallToolResult()` |
| `.toCallToolResult` | `() => CallToolResult` — the MCP `tools/call` response shape: `{content, isError, structuredContent}`, with each content block's `annotations.audience` set correctly |

Full type definitions ship in `index.d.ts`. See the [main repo](https://github.com/orin-dx/michi) for the complete primitive set, the TOON grammar, and the `michi-core` Rust API this wraps (this package always carries the same version as `michi-core`).

## Supported platforms

Prebuilt native binaries: `darwin-arm64`, `linux-x64-musl`. Building from source requires a Rust toolchain (see [`rust-toolchain.toml`](../../rust-toolchain.toml) in the main repo).

## License

FSL-1.1-MIT — Functional Source License, converting to MIT two years after each release. See [`LICENSE`](LICENSE).
