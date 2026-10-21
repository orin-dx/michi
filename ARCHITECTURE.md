# Architecture

This document describes how the `michi` workspace crates fit together.

## Workspace Layout

`michi` is structured as a Cargo workspace with dedicated, single-responsibility sub-crates in `crates/` and a top-level facade crate:

```text
michi (workspace root)
├── crates/
│   ├── michi-truncate    # O(1) UTF-8 safe string truncation (zero dependencies)
│   ├── michi-resilience  # Backoff delay math, RFC 7231 Retry-After parser, FNV-1a idempotency
│   ├── michi-toon        # TOON format renderer, parser, CompactString cells, ToonSerializer
│   ├── michi-core        # Core AXI types: AgentResponse, Audience, Hint, RecoveryHint, DomainError, StatusResponse
│   └── michi-pipeline    # Async pipeline execution (Plan 2): execute_pipeline, execute_pipeline_parallel, CircuitBreaker, CancellationToken
├── michi                 # Facade crate re-exporting all sub-crates (michi-pipeline is NOT re-exported here)
└── packages/michi-node   # Node.js NAPI binding (@orin-dx/michi)
```

## Component Overview

| Crate / Package | Layer | Responsibility | Runtime Dependencies |
| --- | --- | --- | --- |
| `michi-truncate` | Layer 1 | UTF-8 char-boundary content truncation (`floor_char_boundary`). | None |
| `michi-resilience` | Layer 1 | Retry backoff math, `Retry-After` HTTP date parsing, FNV-1a idempotency keys. | None |
| `michi-toon` | Layer 1 | TOON list rendering and parsing. Stack-inlined `CompactString` values for cell optimization. | `compact_str` |
| `michi-core` | Layer 2 | High-level response DTOs (`AgentResponse`, `DomainError`, `StatusResponse`, `CallToolResult`). | `thiserror` (Optional: `serde`, `schemars`, `miette`) |
| `michi-pipeline` | Layer 3 | Async execution over `michi-core`'s `Pipeline` data type: sequential `execute_pipeline`, concurrent `execute_pipeline_parallel`, `CircuitBreaker` resilience, cooperative `CancellationToken`. Own `ExecutionError` type, converts into `DomainError` via `From` — not a variant of `michi_core::Error`. Independently versioned, not re-exported through the facade. | `michi-core`, `michi-resilience`, `tokio` |
| `michi` | Facade | Root re-export facade (`pub use michi_core::*`, `pub use michi_toon::*`, etc.). | `michi-*` workspace crates |
| `packages/michi-node` | Shim | NAPI C-dylib binding for `@orin-dx/michi`. | `napi`, `napi-derive`, `serde_json` |

## Data Composition & Render Flow

`michi` provides two main entry points for formatting output:

1. **`response::AgentResponse` (Fluent Builder)**: Composes TOON lists, KV items, `help[]` hints, and recovery guidance into a unified output buffer. Supports assistant/user audience routing and direct conversion to MCP `CallToolResult`.
2. **Standalone DTOs**: `DomainError`, `StatusResponse`, `AlreadyDone`, and `empty_state()` render themselves directly into agent-readable KV blocks or GitHub Actions annotations without passing through `AgentResponse`.

```mermaid
flowchart TB
    subgraph primitives["Primitives"]
        toon["michi-toon"]
        kv["michi-core::kv"]
        hints["michi-core::hints"]
        recovery["michi-core::recovery"]
        truncate["michi-truncate"]
    end

    toon --> AR
    kv --> AR
    hints --> AR
    recovery --> AR
    truncate --> AR

    AR["michi-core::AgentResponse"]

    AR -->|"render() / render_toon() / render_kv()"| text["Plain Text CLI / Tool Stdout"]
    AR -->|"render_for(Audience)"| textaud["Audience-Routed Output"]
    AR -->|"to_call_tool_result()"| mcpresult["mcp::CallToolResult"]
    mcpresult --> mcpjson["MCP Tools/Call JSON"]

    subgraph standalone["Standalone Renderers"]
        err["DomainError"]
        status["StatusResponse"]
        empty["empty_state()"]
    end

    err --> text
    status --> text
    empty --> text
```

## Node.js / NAPI Bridge (`packages/michi-node`)

The Node.js native addon lives in `packages/michi-node` because Cargo requires a separate `cdylib` target definition:

- Public FFI exports, input validation, and bounds checking live in `src/napi.rs` inside the root crate.
- `packages/michi-node/src/lib.rs` re-exports `src/napi.rs` functions across the crate boundary via `pub use michi::napi::*;`.
- `napi-derive` uses linker sections (`ctor`) to register native functions across crate boundaries without manual wrapper boilerplates.

## NAPI Boundary Contract

The NAPI boundary (`src/napi.rs`) has a specific constructor discipline that differs from pure-Rust callers:

**NAPI functions always use normalizing constructors (`new()`), never strict ones (`try_new()`).**

- `RetryConfig::new()` normalizes out-of-range inputs silently (clamps `jitter_factor`, floors `max_delay`, etc.). `napi.rs`'s resilience-domain call sites use it this way; making it fallible there would require either `unwrap()`/`expect()` (banned) or a silent fallback to `Default` (defeats validation).
- Pure-Rust callers and tests may use `try_new()` / `validate()` for explicit error signals.
- JS-facing functions report errors via `napi::Error`, not `Result` propagation — validation that can't be normalized (truly invalid JS input) becomes a JS exception at the boundary, not a Rust panic.

This asymmetry is intentional. The NAPI layer operates in a "be maximally resilient to JS input, fail loudly only for truly unrecoverable cases" mode; the Rust library layer operates in a "signal problems explicitly through types" mode.

**Error output path:** When you hold a `DomainError`, call `domain_error.to_call_tool_result()` directly. Do not wrap it in `AgentResponse::as_error()` — that double-wraps the render and loses the typed `error`/`retryable` fields from `structured_content`. Both `AgentResponse::to_call_tool_result()` and `DomainError::to_call_tool_result()` produce structurally compatible `structured_content` JSON (same top-level keys).

## WASM & Platform Targets

All core workspace crates (`michi-truncate`, `michi-resilience`, `michi-toon`, `michi-core`) compile for `wasm32-unknown-unknown` and `wasm32-wasip1` targets without OS dependencies.
