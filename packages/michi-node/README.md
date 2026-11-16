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
| `renderToon` | `(opts: ToonOptions) => string` — TOON list rendering |
| `emptyState` | `(typeName: string) => string` — definitive empty-state block |
| `emptyStateWithHints` | `(typeName: string, hints: string[]) => string` — the empty-state block plus `help[N]: …`; like `emptyState`, throws `toon_validation_failed: …` for a `totalCount`/`help`/`recovery` type name |
| `renderHints` | `(hints: string[]) => string` — the inline array `help[N]: h1,h2` |
| `appendHints` | `(body: string, hints: string[]) => string` — append `help[N]: …` to a document, on its own line |
| `renderRecovery` | `(hints: RecoveryHint[]) => string` — the table `recovery[N]{tool,params,reason}:` (`params` is `"{}"` here; an absent reason is `null`) |
| `renderKv` | `(items: KvItem[], totalCount: number | null | undefined, hints: string[]) => string`—`key: value`lines; throws`kv_validation_failed: …`for a repeated key or a`totalCount`/`help`/`recovery` key |
| `renderDomainError` | `(code, message, hints, githubAnnotation?, causes?) => string` — `error`, `message`, `causes`, `exit_code`, `help` |
| `renderStatus` | `(toolName, description, items, hints?) => string` — `tool`, `description`, the table `items[N]{key,value,health,reason}:`, `help` |
| `renderAlreadyDone` | `(operation, summary, hints) => string` — `operation`, `status: already_done`, `summary`, `help` |
| `alreadyDone` | `(stored?: string \| null) => { done: boolean, result?: string }` — classify an idempotency lookup: any stored string (even empty) is done |
| `renderPartialSuccess` | `(completed: OpRef[], failed: FailedOp[], skipped: OpRef[]) => string` — the summary, the `completed[N]{index,operation}:`, `failed[N]{index,operation,reason}:` and `skipped[N]{index,operation,reason}:` tables (`index` is the op's position in your batch; a skipped op's `reason` is a machine token, `null` when absent) and the failed ops' `recovery[N]{tool,params,reason}:` table |
| `renderPipeline` | `(steps: PipelineStep[]) => string` — the table `step[N]{id,name,status,reason}:`; `status` is a `StepStatus` label (`completed`, `outcome_unknown`, …) and `reason` is required exactly for `outcome_unknown`, `skipped` and `not_run` |
| `truncate` | `(content: string, maxChars: number) => { content, originalChars, removed, wasTruncated }` — text that fits comes back unchanged; a longer text keeps its first `maxChars - 1` chars and ends in `…`, and the cut is reported in `removed` and `wasTruncated`, never inside `content`. Add your own hint, for example `cut ${removed} of ${originalChars} chars; pass full=true`, to the response's `help[]`. |
| `parseRetryAfter` | `(headerValue: string) => number \| null` — an RFC 9110 `Retry-After` value (delay-seconds or HTTP-date) in seconds; a past date is `0`, a malformed value `null` |
| `nextRetryDelay` | `(maxRetries, baseDelayMs, maxDelayMs, jitterFactor, jitterSeed, attempt, retryAfterMs?) => number \| null` — the next back-off delay in ms, or `null` once retries are used up; pass a random `jitterSeed` in `[0, 1]` per call |
| `isRetryableStatus` | `(status: number) => boolean` — `true` for 429, 502, 503 and 504; retry 502 and 504 only for idempotent requests |

### `AgentResponse` class

The primary integration point — a builder that composes TOON/KV rendering, hints, and recovery into one response.

Setters return `void`, not `this`, so call them as sequential statements rather than a chain. (Chaining isn't supported across the NAPI boundary — see `docs/spec/04-mcp-and-napi.md` in the main repo for why.)

| Method | Signature |
| --- | --- |
| `constructor` | `(typeName: string)` |
| `.items` | `(rows: ToonValue[][], fields: string[], totalCount?: number \| null) => void` — appends a TOON list section named for the response's type; each call adds one, rendered in call order. `totalCount` is how many rows existed before a cut and renders only when it is larger than the rows passed |
| `.kvItems` | `(items: KvItem[], totalCount?: number \| null) => void` — appends a key-value section the same way |
| `.hint` | `(hint: string) => void` |
| `.recoveryHint` | `(tool: string, reason?: string) => void` |
| `.truncationHint` | `(hint: string) => void` — the caller's instruction for getting a cut cell in full (same as `ToonOptions.truncationHint`). Cut cells end in `…` and are listed in a `truncated[N]{row,field,removed}:` block; when a cell was cut, the hint adds one `help[]` line after your own hints. Without it the block still reports each cut |
| `.asError` | `() => void` |
| `.renderText` | `() => string` — the sections in call order, then `help[]` and `recovery[]` |
| `.renderJson` | `() => string` — compact JSON: `{"body":...,"sections":[...],"hints":[...],"recovery":[...],"isError":bool}` |
| `.renderHintsOnly` | `() => string` — just the `help[N]: …` line |
| `.humanContent` | `(text: string) => void` — attach a `user`-audience companion block for `toCallToolResult()` |
| `.hasHumanContent` | `() => boolean` — whether `.humanContent()` was set |
| `.renderFor` | `(audience: "assistant" \| "user") => string` — `"user"` returns the `humanContent` block if set, otherwise the assistant rendering |
| `.toCallToolResult` | `() => CallToolResult` — the MCP `tools/call` response shape: `{content, isError, structuredContent}`, with each content block's `annotations.audience` set correctly |

Full type definitions ship in `index.d.ts`. See the [main repo](https://github.com/orin-dx/michi) for the complete primitive set, the TOON grammar, and the `michi-core` Rust API this wraps (this package always carries the same version as `michi-core`).

## Input and output limits

Every export that takes arrays or strings rejects oversize input with a thrown `Error` before it does any work (for example `rows length 100001 exceeds maximum of 100000`). The caps:

- 100,000 rows; 1,000 fields, values per row, KV items or status items; 10,000 hints, causes, steps or partial-success entries; 1,000,000 cells per call.
- 1 MiB per cell value, hint, reason, message or step name; 4 KiB per type name, field name, key, tool or identifier.
- **16 MiB of string data** per call, and per `AgentResponse` across all its calls (`input byte total … exceeds maximum of 16777216`, `cumulative byte total … exceeds maximum of 16777216`). This was 64 MiB before it was found to bound only the input: a render writes up to 19 times the bytes it holds (control characters are the worst case), so 64 MiB could produce 1.2 GiB and a `renderJson()` string over V8's limit. At 16 MiB every render stays under 304 MiB, so a response either renders in every format or is rejected up front.

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
