# michi

TypeScript/JavaScript bindings for [`michi`](https://github.com/orin-dx/michi) — AXI response primitives for agent-ergonomic tools. Native binary via NAPI-RS: no WASM, no JS reimplementation of the rendering logic.

## Install

```bash
npm i @orin-dx/michi
```

npm installs the matching prebuilt binary as an optional dependency; see [Supported platforms](#supported-platforms).

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

Every string these functions return for an agent is a TOON 4.4 document with no trailing newline — lists, key-value blocks, errors, status, recovery hints and help blocks alike.

### Free functions

| Function | Signature |
| --- | --- |
| `renderToon` | `(opts: JsToonOptions) => string` — TOON list rendering |
| `emptyState` | `(typeName: string) => string` — definitive empty-state block |
| `emptyStateWithHints` | `(typeName: string, hints: string[]) => string` — the empty-state block plus `help[N]: …`; like `emptyState`, throws `toon_validation_failed: …` for a `totalCount`/`help`/`recovery` type name |
| `renderHints` | `(hints: string[]) => string` — the inline array `help[N]: h1,h2` |
| `appendHints` | `(body: string, hints: string[]) => string` — append `help[N]: …` to a document, on its own line |
| `renderRecovery` | `(hints: JsRecoveryHint[]) => string` — the table `recovery[N]{tool,params,reason}:` (`params` is `"{}"` here; an absent reason is `null`) |
| `renderKv` | `(items: JsKvItem[], totalCount?: number, hints: string[]) => string` — `key: value` lines; throws `kv_validation_failed: …` for a repeated key or a `totalCount`/`help`/`recovery` key |
| `renderDomainError` | `(code, message, hints, githubAnnotation?, causes?) => string` — `error`, `message`, `causes`, `exit_code`, `help` |
| `renderStatus` | `(toolName, description, items, hints?) => string` — `tool`, `description`, the table `items[N]{key,value,health,reason}:`, `help` |
| `renderAlreadyDone` | `(operation, summary, hints) => string` — `operation`, `status: already_done`, `summary`, `help` |
| `renderPartialSuccess` | `(completed: string[], failed: JsFailedOp[], skipped: string[]) => string` — the summary, `completed[N]`, the `failed[N]{operation,reason}:` table, `skipped[N]` and the failed ops' `recovery[N]{tool,params,reason}:` table |
| `renderPipeline` | `(steps: JsPipelineStep[]) => string` — the table `step[N]{id,name,status,reason}:`; `status` is a `StepStatus` label (`completed`, `outcome_unknown`, …) and `reason` is required exactly for `outcome_unknown`, `skipped` and `not_run` |
| `truncate` | `(content: string, maxChars: number, hint: string) => string` — `truncate("a".repeat(200), 50, "full=true")` gives 17 × `a` + `" (183 more chars — use full=true)"`; an empty `hint` drops the `— use …` clause. The marker is plain text that data can imitate: use it for one free-text string, not for TOON cells (`renderToon` reports cuts in a `truncated` block) |
| `parseRetryAfter` | `(headerValue: string) => number \| null` — an RFC 9110 `Retry-After` value (delay-seconds or HTTP-date) in seconds; a past date is `0`, a malformed value `null` |
| `nextRetryDelay` | `(maxRetries, baseDelayMs, maxDelayMs, jitterFactor, jitterSeed, attempt, retryAfterMs?) => number \| null` — the next back-off delay in ms, or `null` once retries are used up; pass a random `jitterSeed` in `[0, 1]` per call |
| `isRetryableStatus` | `(status: number) => boolean` — `true` for 429, 502, 503 and 504; retry 502 and 504 only for idempotent requests |

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
| `.truncationHint` | `(hint: string) => void` — the caller's instruction for getting a cut cell in full (same as `JsToonOptions.truncationHint`). Cut cells end in `…` and are listed in a `truncated[N]{row,field,removed}:` block; when a cell was cut, the hint adds one `help[]` line after your own hints. Without it the block still reports each cut |
| `.asError` | `() => void` |
| `.renderToon` | `() => string` — slot-specific, reads only `items`/`fields` |
| `.renderKv` | `() => string` — slot-specific, reads only `kvItems` |
| `.renderJson` | `() => string` — compact JSON: `{"body":...,"hints":[...],"recovery":[...],"isError":bool}` |
| `.renderHintsOnly` | `() => string` — just the `help[N]: …` line |
| `.humanContent` | `(text: string) => void` — attach a `user`-audience companion block for `toCallToolResult()` |
| `.hasHumanContent` | `() => boolean` — whether `.humanContent()` was set |
| `.renderFor` | `(audience: "assistant" \| "user") => string` — `"user"` returns the `humanContent` block if set, otherwise the assistant rendering |
| `.toCallToolResult` | `() => CallToolResult` — the MCP `tools/call` response shape: `{content, isError, structuredContent}`, with each content block's `annotations.audience` set correctly |

Full type definitions ship in `index.d.ts`. See the [main repo](https://github.com/orin-dx/michi) for the complete primitive set, the TOON grammar, and the `michi-core` Rust API this wraps (this package always carries the same version as `michi-core`).

## Supported platforms

Prebuilt native binaries ship for:

- macOS on Apple silicon (`darwin-arm64`, package `@orin-dx/michi-darwin-arm64`)
- macOS on Intel (`darwin-x64`, package `@orin-dx/michi-darwin-x64`)
- Linux x64 with glibc (`linux-x64-gnu`, package `@orin-dx/michi-linux-x64-gnu`)
- Linux x64 with musl (`linux-x64-musl`, package `@orin-dx/michi-linux-x64-musl`)
- Linux arm64 with glibc (`linux-arm64-gnu`, package `@orin-dx/michi-linux-arm64-gnu`)
- Linux arm64 with musl (`linux-arm64-musl`, package `@orin-dx/michi-linux-arm64-musl`)

Windows isn't supported yet. Building from source needs a Rust toolchain (see [`rust-toolchain.toml`](https://github.com/orin-dx/michi/blob/main/rust-toolchain.toml) in the main repo).

## License

FSL-1.1-MIT — Functional Source License, converting to MIT two years after each release. See [`LICENSE`](LICENSE).
