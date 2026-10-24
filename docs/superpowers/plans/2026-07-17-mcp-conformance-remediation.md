# MCP Conformance Remediation Implementation Plan

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the two MCP protocol non-conformance bugs found by an independent validation pass (wire JSON missing the `"type":"text"` discriminator and using a flat `audience` field instead of nested `annotations.audience[]`; the `serde` feature's direct serialization emitting non-conformant snake_case/PascalCase JSON), close the resulting test-coverage and NAPI-reachability gaps, and reconcile `docs/01-spec.md` (and its peripheral docs) with the MCP integration work that landed but was never documented there.

**Architecture:** Sequential — the wire-format fix (Group A) changes `ContentBlock`'s shape, so everything downstream (the NAPI mirror, new tests, and the doc updates describing the final shape) must land after it, in order. Groups run in one shared worktree; no parallel fan-out, since several tasks touch the same files.

**Tech Stack:** Rust (stable), `serde`/`serde_json` (already-added optional deps from the prior MCP integration pass), `napi`/`napi-derive` v3, `cargo nextest`, `proptest`, `insta`.

---

## Group A: Wire-format conformance (the 2 blocking bugs)

### Task 1: Redesign `ContentBlock`/`CallToolResult` wire shape in `src/mcp.rs`

**Files:**
- Modify: `src/mcp.rs`

Changes `ContentBlock.audience` from a single `Audience` scalar to `Vec<Audience>` (matching MCP's real `annotations.audience` array), and replaces the naive `#[derive(Serialize, Deserialize)]` on `ContentBlock`/`CallToolResult` with a wire-shape conversion so the JSON that comes out actually has `"type":"text"`, nests `audience` under `annotations`, uses camelCase field names, and embeds `structuredContent` as a real JSON value (not a JSON-string-inside-a-string).

- [ ] **Step 1: Write the failing tests**

Replace `src/mcp.rs`'s existing `#[cfg(test)] mod tests` block (everything from `#[cfg(test)]` to the file's closing `}`) with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_block_carries_text_and_audience() {
        let b = ContentBlock { text: "hello".to_string(), audience: vec![Audience::Assistant] };
        assert_eq!(b.text, "hello");
        assert_eq!(b.audience, vec![Audience::Assistant]);
    }

    #[test]
    fn call_tool_result_is_constructible() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "body".to_string(), audience: vec![Audience::Assistant] }],
            is_error: false,
            structured_content: "{}".to_string(),
        };
        assert_eq!(r.content.len(), 1);
        assert!(!r.is_error);
        assert_eq!(r.structured_content, "{}");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn audience_serializes_and_deserializes() {
        let a = Audience::Assistant;
        let json = serde_json::to_string(&a).expect("serializes");
        let back: Audience = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(a, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn audience_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Audience::Assistant).expect("serializes"), "\"assistant\"");
        assert_eq!(serde_json::to_string(&Audience::User).expect("serializes"), "\"user\"");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn call_tool_result_serializes_and_deserializes() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "body".to_string(), audience: vec![Audience::Assistant] }],
            is_error: false,
            structured_content: "{}".to_string(),
        };
        let json = serde_json::to_string(&r).expect("serializes");
        let back: CallToolResult = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(r, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn call_tool_result_wire_json_matches_mcp_shape() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "hi".to_string(), audience: vec![Audience::Assistant] }],
            is_error: false,
            structured_content: "{}".to_string(),
        };
        let json = serde_json::to_string(&r).expect("serializes");
        assert_eq!(
            json,
            r#"{"content":[{"type":"text","text":"hi","annotations":{"audience":["assistant"]}}],"isError":false,"structuredContent":{}}"#
        );
    }

    #[test]
    #[cfg(feature = "serde")]
    fn call_tool_result_structured_content_round_trips_as_object_not_double_encoded_string() {
        let r = CallToolResult {
            content: vec![],
            is_error: false,
            structured_content: r#"{"totalCount":3}"#.to_string(),
        };
        let json = serde_json::to_string(&r).expect("serializes");
        // structuredContent must be an embedded object — assert the raw JSON has an
        // unescaped, nested object, not a JSON string containing escaped quotes.
        assert!(json.contains(r#""structuredContent":{"totalCount":3}"#), "got: {json}");
        let back: CallToolResult = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back.structured_content, r#"{"totalCount":3}"#);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p michi --features serde mcp::tests`
Expected: FAIL — compile errors, since `ContentBlock.audience` is still a scalar `Audience`, not `Vec<Audience>`, and the wire shape doesn't match yet.

- [ ] **Step 3: Rewrite the type definitions and add the wire conversion**

Replace `src/mcp.rs`'s content from the top of the file through the end of the `CallToolResult` struct definition (i.e. everything before `#[cfg(test)]`) with:

```rust
//! MCP `CallToolResult` mapping — the shape a tool call actually returns to
//! an MCP client. Always compiled: this is pure struct construction, no new
//! dependencies, so there's no reason to gate it behind a feature flag.
//!
//! michi does not know about the rest of the MCP protocol (no JSON-RPC, no
//! tool registration, no server bootstrapping, no `outputSchema` validation —
//! see `docs/01-spec.md`'s Non-goals). This module owns exactly one thing:
//! turning an already-built [`crate::response::AgentResponse`] into the
//! `content`/`isError`/`structuredContent` shape MCP's `tools/call` response
//! expects — including the real wire-format details (the `"type": "text"`
//! discriminator, `annotations.audience` nesting, camelCase field names)
//! under the `serde` feature and the NAPI boundary, not just an internal
//! Rust-shaped approximation of them.

/// Which surface a [`ContentBlock`] is meant for. Mirrors MCP's
/// `annotations.audience` — an array in the real protocol because one block
/// can target more than one audience; michi always populates exactly one
/// element per block today (see [`ContentBlock::audience`]), but the field
/// is a `Vec` so no translation is needed at the serialization boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Audience {
    /// The compact, token-efficient surface — what michi renders today.
    Assistant,
    /// A human-readable surface, supplied by the caller. michi does not
    /// generate this text itself (see this crate's Non-goals: no
    /// display-format Markdown) — it only carries it correctly through to
    /// the protocol shape when a caller has one.
    User,
}

/// One text content block. Wire-conformant with MCP's text content shape —
/// `{"type": "text", "text": "...", "annotations": {"audience": [...]}}` —
/// under the `serde` feature; NAPI's `JsContentBlock` (`src/napi.rs`)
/// produces the identical shape independently for JS callers.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "ContentBlockWire", from = "ContentBlockWire"))]
pub struct ContentBlock {
    /// The block's text content.
    pub text: String,
    /// Which surface(s) this block is meant for. Always one element today
    /// (assistant XOR user) — a `Vec` because MCP's `annotations.audience`
    /// is an array; see this type's own doc comment.
    pub audience: Vec<Audience>,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnnotationsWire {
    audience: Vec<Audience>,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct ContentBlockWire {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    annotations: AnnotationsWire,
}

#[cfg(feature = "serde")]
impl From<ContentBlock> for ContentBlockWire {
    fn from(b: ContentBlock) -> Self {
        Self { kind: "text".to_string(), text: b.text, annotations: AnnotationsWire { audience: b.audience } }
    }
}

#[cfg(feature = "serde")]
impl From<ContentBlockWire> for ContentBlock {
    fn from(w: ContentBlockWire) -> Self {
        Self { text: w.text, audience: w.annotations.audience }
    }
}

/// The MCP `CallToolResult` shape: what a tool call returns to a client.
/// Built via [`crate::response::AgentResponse::to_call_tool_result`], never
/// hand-constructed by a caller.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "CallToolResultWire", from = "CallToolResultWire"))]
pub struct CallToolResult {
    /// Text content blocks — the primary `assistant`-audience block first,
    /// then an optional `user`-audience block if the caller supplied one.
    pub content: Vec<ContentBlock>,
    /// Whether this is a tool execution error, per MCP's error-reporting
    /// model (`isError: true` in the result, not a JSON-RPC protocol error —
    /// see MCP's tools spec, "Tool Execution Errors").
    pub is_error: bool,
    /// The same data as `content[0]`, as a JSON string — MCP's
    /// `structuredContent` companion. Always populated: michi already builds
    /// this JSON for `AgentResponse::render(OutputFormat::Json)`. Only
    /// `hints`/`recovery`/`isError` are genuinely structured within it —
    /// `body` (the rendered TOON/KV text, including `totalCount`) stays a
    /// single embedded string, so a JSON-aware client gains structure over
    /// `content[0].text` for the former but still has to parse the latter.
    /// Populated unconditionally, even for a tool with a declared
    /// `outputSchema` this generic shape won't conform to — michi has no
    /// visibility into any `outputSchema` (confirmed non-goal), so a caller
    /// with one should substitute their own conforming payload instead of
    /// using this field as-is. Kept as a plain `String` (not
    /// `serde_json::Value`) so this always-compiled module doesn't need
    /// `serde_json` as a mandatory dependency; the `serde` feature and NAPI
    /// boundary each independently convert it to a real embedded JSON value
    /// on the wire (see [`CallToolResultWire`], and `src/napi.rs`).
    pub structured_content: String,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CallToolResultWire {
    content: Vec<ContentBlock>,
    is_error: bool,
    structured_content: serde_json::Value,
}

#[cfg(feature = "serde")]
impl From<CallToolResult> for CallToolResultWire {
    fn from(r: CallToolResult) -> Self {
        let structured_content = serde_json::from_str(&r.structured_content).unwrap_or(serde_json::Value::Null);
        Self { content: r.content, is_error: r.is_error, structured_content }
    }
}

#[cfg(feature = "serde")]
impl From<CallToolResultWire> for CallToolResult {
    fn from(w: CallToolResultWire) -> Self {
        let structured_content = serde_json::to_string(&w.structured_content).unwrap_or_default();
        Self { content: w.content, is_error: w.is_error, structured_content }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p michi --features serde mcp::tests`
Expected: all pass, including the new wire-shape and round-trip tests.

Run: `cargo nextest run -p michi mcp::tests` (no `serde` feature — confirms the always-compiled, non-serde-gated tests still pass without it)
Expected: `content_block_carries_text_and_audience` and `call_tool_result_is_constructible` pass; the `#[cfg(feature = "serde")]` tests don't run at all (not a failure).

- [ ] **Step 5: Run full verification and commit**

Run: `cargo clippy -p michi --features serde -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

Run: `cargo build -p michi` (default, no features)
Expected: clean — confirms `src/mcp.rs` is still always-compiled with zero new dependencies in the default build (the `AnnotationsWire`/`ContentBlockWire`/`CallToolResultWire` types and their `impl From` are entirely `#[cfg(feature = "serde")]`-gated).

```bash
git add src/mcp.rs
git commit -m "fix(mcp): make ContentBlock/CallToolResult wire-conformant with real MCP shape"
```

---

### Task 2: Update `AgentResponse::to_call_tool_result()` for the new `Vec<Audience>` shape

**Files:**
- Modify: `src/response.rs`

- [ ] **Step 1: Update the two existing tests that assert a scalar `audience`**

In `src/response.rs`'s test module, find:
```rust
    #[test]
    fn to_call_tool_result_uses_render_text_as_assistant_block() {
        let r = AgentResponse::new("issue").kv_items(vec![KvItem { key: "id".into(), value: KvValue::Int(1) }]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].text, r.render_kv());
        assert_eq!(result.content[0].audience, crate::mcp::Audience::Assistant);
        assert!(!result.is_error);
    }
```
Change the audience assertion to:
```rust
        assert_eq!(result.content[0].audience, vec![crate::mcp::Audience::Assistant]);
```

Find:
```rust
    #[test]
    fn to_call_tool_result_includes_human_content_block_when_set() {
        let r = AgentResponse::new("t").kv_items(vec![]).human_content("Here's a friendly summary.");
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].text, "Here's a friendly summary.");
        assert_eq!(result.content[1].audience, crate::mcp::Audience::User);
    }
```
Change the audience assertion to:
```rust
        assert_eq!(result.content[1].audience, vec![crate::mcp::Audience::User]);
```

- [ ] **Step 2: Add the TOON-path test (the design doc's Testing section required both paths; only KV existed)**

Add to the test module, near the other `to_call_tool_result_*` tests:
```rust
    #[test]
    fn to_call_tool_result_uses_render_text_as_assistant_block_for_toon_path() {
        let r = AgentResponse::new("issue")
            .items(vec![vec![Value::Int(1), Value::Str("open".to_string())]], &["id", "state"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].text, r.render_toon());
        assert_eq!(result.content[0].audience, vec![crate::mcp::Audience::Assistant]);
    }
```

- [ ] **Step 3: Run test to verify it fails, then update the implementation**

Run: `cargo nextest run -p michi response::tests::to_call_tool_result`
Expected: FAIL — compile error, `ContentBlock { audience: crate::mcp::Audience::Assistant, .. }` in `to_call_tool_result()`'s body no longer type-checks against `Vec<Audience>`.

In `src/response.rs`, find:
```rust
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        let mut content =
            vec![crate::mcp::ContentBlock { text: self.render_text(), audience: crate::mcp::Audience::Assistant }];
        if let Some(human) = &self.human_content {
            content.push(crate::mcp::ContentBlock { text: human.clone(), audience: crate::mcp::Audience::User });
        }
        crate::mcp::CallToolResult {
            content,
            is_error: self.is_error,
            structured_content: self.render(OutputFormat::Json),
        }
    }
```
Replace with:
```rust
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        let mut content = vec![crate::mcp::ContentBlock {
            text: self.render_text(),
            audience: vec![crate::mcp::Audience::Assistant],
        }];
        if let Some(human) = &self.human_content {
            content.push(crate::mcp::ContentBlock { text: human.clone(), audience: vec![crate::mcp::Audience::User] });
        }
        crate::mcp::CallToolResult {
            content,
            is_error: self.is_error,
            structured_content: self.render(OutputFormat::Json),
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p michi response::tests`
Expected: all pass, including the updated and new tests.

- [ ] **Step 5: Run full verification and commit**

Run: `cargo nextest run -p michi --lib && cargo clippy -p michi --all-features -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/response.rs
git commit -m "fix(response): to_call_tool_result() uses Vec<Audience>, add TOON-path coverage"
```

> **Note on actual commit history (added post-review):** Steps 1 and 3 above (the
> `Vec<Audience>` fix to `to_call_tool_result()`'s body and the two updated test
> assertions) landed one commit earlier than planned, in `bb757f9` ("fix: address
> review findings for task 1 (response.rs Vec<Audience> compile break)"), because
> that fix was needed to unbreak Task 1's own build before Task 2 could even compile.
> The commit carrying this task's exact planned message (`ef9127e`) contains only
> Step 2 (the new TOON-path test — 10 insertions). The net working tree at the end
> of both commits matches this task's spec; only the commit boundary/message
> attribution differs from what's written above. Left as history (not rewritten) per
> this repo's no-amend/no-force-push policy — see the task-2 review-fix commit for
> the recorded discrepancy.

---

### Task 3: Update NAPI `JsContentBlock`/`JsCallToolResult` for the real MCP shape

**Files:**
- Modify: `src/napi.rs`
- Test: same file (Rust side) + `packages/michi-node/__test__/index.test.mjs` (JS side)

- [ ] **Step 1: Write the failing tests (Rust side)**

In `src/napi.rs`'s test module, find the 3 existing `to_call_tool_result` tests and replace them with:
```rust
    #[test]
    fn js_agent_response_to_call_tool_result_basic() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("int") }]).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].content_type, "text");
        assert_eq!(result.content[0].annotations.audience, vec!["assistant".to_string()]);
        assert!(!result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_reflects_is_error() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.as_error().unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_structured_content_is_parsed_json() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.structured_content.get("isError").is_some(), "got: {:?}", result.structured_content);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_includes_user_block_with_correct_annotations() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.human_content("friendly summary".to_string()).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].content_type, "text");
        assert_eq!(result.content[1].annotations.audience, vec!["user".to_string()]);
    }
```

Note: the last test above calls `r.human_content(...)`, which does not exist yet — it's added in Task 4. Write it now (per this task's spec) and let Task 4 make it pass; alternatively, if executing tasks strictly one at a time, skip adding this specific test until Task 4 lands and add it there instead. Either ordering is fine as long as it exists once both tasks are done.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p michi --features napi napi::tests::js_agent_response_to_call_tool_result`
Expected: FAIL — `JsCallToolResult`'s `content` field elements have no `content_type`/`annotations` members yet.

- [ ] **Step 3: Rewrite `JsContentBlock`/`JsCallToolResult` and `to_call_tool_result()`**

In `src/napi.rs`, find:
```rust
/// One MCP content block (JavaScript-friendly).
#[napi(object)]
pub struct JsContentBlock {
    /// The block's text content.
    pub text: String,
    /// `"assistant"` or `"user"` — which surface this block is meant for.
    pub audience: String,
}
```
Replace with:
```rust
/// MCP content-block annotations (JavaScript-friendly). Currently carries
/// only `audience` — michi has no concept of MCP's optional `priority`.
#[napi(object)]
pub struct JsAnnotations {
    /// `["assistant"]` or `["user"]` — which surface(s) this block targets.
    pub audience: Vec<String>,
}

/// One MCP content block (JavaScript-friendly). Wire-conformant with MCP's
/// text content shape: `{type: "text", text, annotations: {audience: [...]}}`.
#[napi(object)]
pub struct JsContentBlock {
    /// Always `"text"` — michi only ever produces text content blocks.
    #[napi(js_name = "type")]
    pub content_type: String,
    /// The block's text content.
    pub text: String,
    /// Which surface(s) this block is meant for.
    pub annotations: JsAnnotations,
}
```

Find `to_call_tool_result()`'s body:
```rust
    pub fn to_call_tool_result(&self) -> napi::Result<JsCallToolResult> {
        let inner = self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))?;
        let result = inner.to_call_tool_result();
        let structured_content = serde_json::from_str(&result.structured_content)
            .map_err(|e| napi::Error::from_reason(format!("structured_content was not valid JSON: {e}")))?;
        Ok(JsCallToolResult {
            content: result
                .content
                .into_iter()
                .map(|c| JsContentBlock {
                    text: c.text,
                    audience: match c.audience {
                        crate::mcp::Audience::Assistant => "assistant".to_string(),
                        crate::mcp::Audience::User => "user".to_string(),
                    },
                })
                .collect(),
            is_error: result.is_error,
            structured_content,
        })
    }
```
Replace with:
```rust
    pub fn to_call_tool_result(&self) -> napi::Result<JsCallToolResult> {
        let inner = self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))?;
        let result = inner.to_call_tool_result();
        let structured_content = serde_json::from_str(&result.structured_content)
            .map_err(|e| napi::Error::from_reason(format!("structured_content was not valid JSON: {e}")))?;
        Ok(JsCallToolResult {
            content: result
                .content
                .into_iter()
                .map(|c| JsContentBlock {
                    content_type: "text".to_string(),
                    text: c.text,
                    annotations: JsAnnotations {
                        audience: c
                            .audience
                            .into_iter()
                            .map(|a| match a {
                                crate::mcp::Audience::Assistant => "assistant".to_string(),
                                crate::mcp::Audience::User => "user".to_string(),
                            })
                            .collect(),
                    },
                })
                .collect(),
            is_error: result.is_error,
            structured_content,
        })
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p michi --features napi napi::tests`
Expected: all pass (the `..._includes_user_block_with_correct_annotations` test only passes once Task 4's `human_content()` setter also exists — if running Task 3 in isolation before Task 4, this one test is expected to fail to compile; treat that as acceptable and proceed to Task 4 immediately, or defer writing that one test until Task 4 per Step 1's note).

- [ ] **Step 5: Update the JS integration test**

In `packages/michi-node/__test__/index.test.mjs`, find the `toCallToolResult` describe block and replace its contents with:
```javascript
void describe('toCallToolResult', () => {
  void it('returns MCP-conformant content blocks with type/annotations.audience', () => {
    const r = new AgentResponse('issue')
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 1 } }])
    const result = r.toCallToolResult()
    assert.strictEqual(result.content.length, 1)
    assert.strictEqual(result.content[0].type, 'text')
    assert.deepStrictEqual(result.content[0].annotations.audience, ['assistant'])
    assert.strictEqual(result.isError, false)
    assert.strictEqual(typeof result.structuredContent, 'object')
    assert.strictEqual(result.structuredContent.isError, false)
  })

  void it('reflects isError and includes a user-audience block from humanContent', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    r.asError()
    r.humanContent('friendly summary')
    const result = r.toCallToolResult()
    assert.strictEqual(result.isError, true)
    assert.strictEqual(result.structuredContent.isError, true)
    assert.strictEqual(result.content.length, 2)
    assert.strictEqual(result.content[1].type, 'text')
    assert.deepStrictEqual(result.content[1].annotations.audience, ['user'])
  })
})
```
(this test calls `r.humanContent(...)`, added in Task 4 — same note as Step 1 applies.)

- [ ] **Step 6: Rebuild the NAPI binary and run the full test suite once Task 4 also lands**

Run: `cd packages/michi-node && pnpm build --platform && pnpm test`
Expected: all pass. If running Task 3 strictly before Task 4, `humanContent` will not exist yet — either complete Task 4 first, or comment out the `r.humanContent(...)` line and the two assertions that depend on it temporarily, then restore them in Task 4.

- [ ] **Step 7: Run full verification and commit**

Run: `cargo clippy -p michi --features napi -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/napi.rs packages/michi-node/__test__/index.test.mjs packages/michi-node/index.d.ts packages/michi-node/index.js
git commit -m "fix(napi): JsContentBlock matches real MCP shape (type + annotations.audience)"
```

---

## Group B: NAPI `humanContent()` setter

### Task 4: Add `JsAgentResponse::humanContent()`

**Files:**
- Modify: `src/napi.rs`
- Modify: `packages/michi-node/README.md`
- Test: same files + `packages/michi-node/__test__/index.test.mjs`

Closes the reachability gap the validation found: the two-block `CallToolResult` shape (assistant + optional user block) was designed in the prior MCP integration pass, but nothing let a TypeScript caller populate the user-audience block.

- [ ] **Step 1: Write the failing test**

In `src/napi.rs`'s test module, add (if not already present from Task 3, Step 1's forward-reference):
```rust
    #[test]
    fn js_agent_response_human_content_adds_user_audience_block() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.human_content("friendly summary".to_string()).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].text, "friendly summary");
        assert_eq!(result.content[1].annotations.audience, vec!["user".to_string()]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --features napi js_agent_response_human_content`
Expected: FAIL — no `human_content` method on `JsAgentResponse`.

- [ ] **Step 3: Add the setter**

In `src/napi.rs`, add to `impl JsAgentResponse`, immediately after `as_error()`:
```rust
    /// Attach a human-facing companion block (`audience: user`) for MCP
    /// callers. Optional — most callers won't set this.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn human_content(&mut self, text: String) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.human_content(text));
        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p michi --features napi napi::tests`
Expected: all pass, including this task's test and the Task 3 test that referenced `human_content`/`humanContent`.

- [ ] **Step 5: Confirm the JS test from Task 3, Step 5 now passes**

Run: `cd packages/michi-node && pnpm build --platform && pnpm test`
Expected: all pass, including `toCallToolResult`'s `humanContent` assertions.

- [ ] **Step 6: Update the npm README's method table**

In `packages/michi-node/README.md`, find the `AgentResponse` class method table and add two rows — `toCallToolResult` and `humanContent` — matching the existing table's style. Find:
```markdown
| `.renderHintsOnly` | `() => string` — just the `help[N]:` block |
```
Change to:
```markdown
| `.renderHintsOnly` | `() => string` — just the `help[N]:` block |
| `.humanContent` | `(text: string) => void` — attach a `user`-audience companion block for `toCallToolResult()` |
| `.toCallToolResult` | `() => CallToolResult` — the MCP `tools/call` response shape: `{content, isError, structuredContent}`, with each content block's `annotations.audience` set correctly |
```

- [ ] **Step 7: Run full verification and commit**

Run: `cargo clippy -p michi --features napi -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/napi.rs packages/michi-node/__test__/index.test.mjs packages/michi-node/index.d.ts packages/michi-node/index.js packages/michi-node/README.md
git commit -m "feat(napi): add humanContent() setter, closing the user-audience-block reachability gap"
```

---

## Group C: Test coverage (snapshot + property test)

### Task 5: Add an insta snapshot test and a proptest for `CallToolResult`

**Files:**
- Modify: `tests/snapshot_tests.rs`
- Create: `tests/proptest_mcp.rs`

- [ ] **Step 1: Add the snapshot test**

`tests/snapshot_tests.rs` already imports `AgentResponse` (`use michi::response::{AgentResponse, OutputFormat};`) — no new import needed.

Add a new test function, near the other snapshot tests:
```rust
#[test]
fn snapshot_call_tool_result_kv() {
    let r = AgentResponse::new("issue")
        .kv_items(vec![
            KvItem { key: "id".into(), value: KvValue::Text("abc-123".into()) },
            KvItem { key: "state".into(), value: KvValue::Text("open".into()) },
        ])
        .human_content("Issue abc-123 is currently open.");
    let result = r.to_call_tool_result();
    insta::assert_debug_snapshot!(result);
}
```

- [ ] **Step 2: Run test to generate and review the snapshot**

Run: `cargo nextest run -p michi --test snapshot_tests snapshot_call_tool_result_kv`
Expected: FAIL the first time (no snapshot exists yet — insta creates a `.snap.new` file).

Run: `cargo insta review` (or `just snapshots`, per this project's `justfile`)
Expected: shows the new snapshot's content — a `CallToolResult` debug-formatted with two content blocks (assistant KV render, then the human-content user block), `is_error: false`, and `structured_content` as the JSON string. Accept it.

Run: `cargo nextest run -p michi --test snapshot_tests snapshot_call_tool_result_kv`
Expected: PASS.

- [ ] **Step 3: Create the property test file**

Create `tests/proptest_mcp.rs`:
```rust
#![cfg(feature = "serde")]

use michi::mcp::{Audience, CallToolResult, ContentBlock};
use proptest::prelude::*;

fn audience_strategy() -> impl Strategy<Value = Audience> {
    prop_oneof![Just(Audience::Assistant), Just(Audience::User)]
}

fn content_block_strategy() -> impl Strategy<Value = ContentBlock> {
    ("[a-zA-Z0-9 ]{0,40}", proptest::collection::vec(audience_strategy(), 1..3))
        .prop_map(|(text, audience)| ContentBlock { text, audience })
}

fn call_tool_result_strategy() -> impl Strategy<Value = CallToolResult> {
    (proptest::collection::vec(content_block_strategy(), 1..3), any::<bool>())
        .prop_map(|(content, is_error)| CallToolResult { content, is_error, structured_content: "{}".to_string() })
}

proptest! {
    #[test]
    fn call_tool_result_round_trips_through_json(r in call_tool_result_strategy()) {
        let json = serde_json::to_string(&r).expect("serializes");
        let back: CallToolResult = serde_json::from_str(&json).expect("deserializes");
        prop_assert_eq!(r, back);
    }

    #[test]
    fn call_tool_result_wire_json_is_mcp_conformant(r in call_tool_result_strategy()) {
        let json = serde_json::to_string(&r).expect("serializes");
        prop_assert!(json.contains(r#""type":"text""#), "missing type discriminator: {json}");
        prop_assert!(json.contains("\"isError\""), "missing camelCase isError: {json}");
        prop_assert!(json.contains("\"structuredContent\""), "missing camelCase structuredContent: {json}");
        prop_assert!(!json.contains("\"is_error\""), "leaked snake_case is_error: {json}");
    }
}
```

Note: `#![cfg(feature = "serde")]` as the file's first line makes the whole file compile to an empty test binary when the `serde` feature is off, so `cargo test`/`cargo nextest run` without `--features serde` still succeeds (no `[[test]] required-features` entry needed in `Cargo.toml`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p michi --features serde --test proptest_mcp`
Expected: both property tests pass (proptest runs 256 cases each by default).

Run: `cargo nextest run -p michi --test proptest_mcp` (no `serde` feature)
Expected: PASS trivially — zero tests ran (file is empty under this feature set), not a failure.

- [ ] **Step 5: Run full verification and commit**

Run: `cargo nextest run -p michi --all-features && cargo clippy -p michi --all-features -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add tests/snapshot_tests.rs tests/proptest_mcp.rs tests/snapshots
git commit -m "test(mcp): add insta snapshot and proptest coverage for CallToolResult"
```

---

## Group D: `docs/01-spec.md` reconciliation

### Task 6: Update Non-goals, Consumer map, and the "stays in your application" table

**Files:**
- Modify: `docs/01-spec.md`

- [ ] **Step 1: Fix the stale Non-goals claim**

In `docs/01-spec.md`, find:
```
- **MCP protocol knowledge** — no `content[]`, no `outputSchema`, no
  `structuredContent`. Those are assembled by the calling MCP framework.
```
Change to:
```
- **Full MCP protocol knowledge** — no JSON-RPC, no tool registration, no
  server bootstrapping, no `outputSchema` validation. `AgentResponse::to_call_tool_result()`
  (and the NAPI `toCallToolResult()`) *do* assemble the `content[]`/`isError`/
  `structuredContent` shape from an already-built response — see the `mcp`
  module section below — but nothing beyond that single struct-assembly step.
```

- [ ] **Step 2: Fix the same stale claim in the "stays in your application" table**

Find:
```
| MCP `content[]` assembly | Protocol knowledge |
```
Change to:
```
| Full MCP protocol (JSON-RPC, registration, bootstrapping) | Protocol knowledge beyond struct assembly |
```

- [ ] **Step 3: Update the "genuinely new" table**

Find:
```
| `render_hints_only()` | Append hints to an existing body without re-rendering |
```
Change to:
```
| `render_hints_only()` | Append hints to an existing body without re-rendering |
| `mcp::CallToolResult` + `AgentResponse::to_call_tool_result()` | Assembles a wire-conformant MCP `tools/call` response from an already-built `AgentResponse` — no prior shared primitive did this |
```

- [ ] **Step 4: Verify and commit**

Run: `grep -n "assembled by the calling MCP framework\|MCP .content..* assembly" docs/01-spec.md`
Expected: no output (both stale claims replaced).

```bash
git add docs/01-spec.md
git commit -m "docs(spec): correct stale MCP content[] non-goal claims now that to_call_tool_result() exists"
```

---

### Task 7: Update the Cargo.toml section (features, dependencies, deviation rationale)

**Files:**
- Modify: `docs/01-spec.md`

- [ ] **Step 1: Update the Plan-2 disclaimer note**

Find:
```
> Deviations from earlier drafts of this section are tracked in
> `docs/superpowers/plans/2026-07-08-spec-parity.md`'s Design Decisions table.
> This section shows the actual, shipped Plan 1 (pure-primitives) surface —
> the workspace `Cargo.toml` also carries a `pipeline`/`fuzzy`/`cache`/`cli`/
> `mcp`/`full` feature set for the execution layer (Plan 2), which is out of
> scope for this spec.
```
Change to:
```
> Deviations from earlier drafts of this section are tracked in
> `docs/superpowers/plans/2026-07-08-spec-parity.md`'s Design Decisions table.
> This section shows the actual, shipped Plan 1 (pure-primitives) surface —
> the workspace `Cargo.toml` also carries a `pipeline`/`fuzzy`/`cache`/`cli`/
> `full` feature set for the execution layer (Plan 2), which is out of scope
> for this spec. `serde` is *not* a Plan 2 feature, despite being adjacent in
> the same `[features]` table — it gates `Serialize`/`Deserialize` on Plan 1's
> own pure-primitive types (`Value`, `KvValue`, `Hint`, `RecoveryHint`, the
> `mcp` module's types) plus the `toon::list()` convenience function, so it's
> documented below alongside `napi`. The `mcp` Cargo feature that appeared in
> an earlier draft of this workspace was retired — `mcp` is now an
> always-compiled module (see the `mcp` module section below), not a feature
> flag.
```

- [ ] **Step 2: Update the `[features]`/`[dependencies]` code block**

Find:
```toml
[features]
default = []
napi    = ["dep:napi", "dep:napi-derive"]
cli     = []  # reserved: terminal-width-aware rendering, colour support

[dependencies]
thiserror  = "2"

[dependencies.napi]
version  = "3"
features = ["napi6"]
optional = true

[dependencies.napi-derive]
version  = "3"
optional = true

[dev-dependencies]
divan    = "0.1"
proptest = "1"
insta    = { version = "1", features = ["yaml"] }
```
Change to:
```toml
[features]
default = []
napi    = ["dep:napi", "dep:napi-derive", "dep:serde_json"]
serde   = ["dep:serde", "dep:serde_json"]
cli     = []  # reserved: terminal-width-aware rendering, colour support

[dependencies]
thiserror  = "2"

[dependencies.napi]
version  = "3"
features = ["napi6", "serde-json"]
optional = true

[dependencies.napi-derive]
version  = "3"
optional = true

[dependencies.serde]
version  = "1"
features = ["derive"]
optional = true

[dependencies.serde_json]
version  = "1"
features = ["preserve_order"]
optional = true

[dev-dependencies]
divan    = "0.1"
proptest = "1"
insta    = { version = "1", features = ["yaml"] }
```

- [ ] **Step 3: Rewrite the "deliberately absent" paragraph**

Find:
```
`serde`/`serde_json` are deliberately absent — an earlier draft listed them as
unconditional dependencies, which an adversarial review found unused outside
NAPI-boundary conversions and removed, restoring the "zero deps by default"
guarantee this crate promises. `kv::KvValue` (typed scalar enum) fills the
same role `serde_json::Value` would have, at zero dependency cost. Benchmarks
use `divan`, not `criterion`, per this crate's own non-negotiables.
```
Change to:
```
`serde`/`serde_json` are absent from the *default* build — an earlier draft
listed them as unconditional dependencies, which an adversarial review found
unused outside NAPI-boundary conversions and removed, restoring the "zero
deps by default" guarantee this crate promises. `kv::KvValue` (typed scalar
enum) fills the same role `serde_json::Value` would have for every consumer
who doesn't opt in, at zero dependency cost. `serde_json` is pulled in by
either the `napi` feature (typed `structuredContent`, wire-conformant
`ContentBlock`/`CallToolResult` conversion) or the `serde` feature
(`Serialize`/`Deserialize` on `Value`/`KvValue`/`Hint`/`RecoveryHint`/the
`mcp` types, plus `toon::list()`) — never both unconditionally, and never in
a build with neither feature enabled. `preserve_order` is enabled on
`serde_json` so `toon::list()`'s field order follows each struct's declared
field order rather than being alphabetized. Benchmarks use `divan`, not
`criterion`, per this crate's own non-negotiables.
```

- [ ] **Step 4: Update the "Why napi v3" section's zero-serde_json claim**

Find (in the "## NAPI npm package" section, not the Cargo.toml section):
```
- Typed `#[napi(object)]` structs (`JsToonValue`, `JsKvItem`, ...) for the
  dynamic FFI boundary (cell values, recovery params), not `serde_json::Value`
  — same zero-`serde_json` rationale as the rest of this spec
```
Change to:
```
- Typed `#[napi(object)]` structs (`JsToonValue`, `JsKvItem`, ...) for the
  dynamic FFI boundary (cell values, recovery params) — kept as tagged Rust
  enums converted by hand, not `serde_json::Value`, even though the `napi`
  feature does pull in `serde_json` (for `toCallToolResult()`'s typed
  `structuredContent` — see the `mcp` module section). The two are
  independent choices: `serde_json` is available once `napi` is enabled, but
  `JsToonValue`/`JsKvItem` stay their own explicit shape because a `Value`
  would accept malformed input silently instead of matching one of the
  documented variants.
```

- [ ] **Step 5: Verify and commit**

Run: `grep -n "deliberately absent\|zero-.serde_json. rationale" docs/01-spec.md`
Expected: no output (both replaced).

Run: `grep -n '"serde"' docs/01-spec.md`
Expected: at least one match, inside the updated `[features]` code block.

```bash
git add docs/01-spec.md
git commit -m "docs(spec): reconcile Cargo.toml section with the real serde/napi feature graph"
```

---

### Task 8: Update Crate layout, `response` module section, NAPI section, Feature flags section, and Versioning claim

**Files:**
- Modify: `docs/01-spec.md`

- [ ] **Step 1: Add `mcp.rs` to the Crate layout listing**

Find:
```
    resilience/
      mod.rs                     # RetryConfig, parse_retry_after(), next_retry_delay()
      circuit.rs, policy.rs      # pipeline-feature-gated (Plan 2) — out of scope here
    status.rs                   # StatusItem, StatusResponse, Health
```
Change to:
```
    resilience/
      mod.rs                     # RetryConfig, parse_retry_after(), next_retry_delay()
      circuit.rs, policy.rs      # pipeline-feature-gated (Plan 2) — out of scope here
    status.rs                   # StatusItem, StatusResponse, Health
    mcp.rs                      # Audience, ContentBlock, CallToolResult — always compiled
```

- [ ] **Step 2: Add a `mcp` module section**

Immediately before the `## NAPI npm package — michin` heading, insert a new section:

```markdown
## `mcp` module

Always compiled — no feature gate, no new dependency in the default build.
Owns exactly one thing: turning an already-built `AgentResponse` into the
shape MCP's `tools/call` response expects.

```rust
pub enum Audience {
    Assistant,
    User,
}

pub struct ContentBlock {
    pub text: String,
    pub audience: Vec<Audience>,   // MCP's annotations.audience is an array
}

pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    pub is_error: bool,
    pub structured_content: String,   // JSON text; see field doc for what's actually structured
}
```

`audience` is a `Vec`, not a scalar, because MCP's real `annotations.audience`
is an array (one block can target more than one audience) — michi always
populates exactly one element per block today, but the type matches the wire
shape so no translation is needed at the serialization boundary.

Under the `serde` feature, `ContentBlock`/`CallToolResult` do **not** derive
`Serialize`/`Deserialize` directly onto their Rust-shaped fields — a naive
derive would emit `is_error`/`structured_content` (snake_case) and a bare
`audience` field, neither of which is valid MCP JSON. Instead each type
converts through a private `*Wire` struct (`serde(into = "...", from = "...")`)
that adds the `"type": "text"` discriminator, nests `audience` under
`annotations`, renames fields to camelCase, and — for `CallToolResult` —
parses `structured_content` into a real embedded JSON value rather than a
double-encoded string. `Audience` itself serializes as lowercase
(`"assistant"`/`"user"`), matching MCP's `Role` type.

`AgentResponse::to_call_tool_result()` (see the `response` module section)
is the only intended constructor — it builds the primary `assistant`-audience
block from whichever of `render_toon()`/`render_kv()` is active, an optional
second `user`-audience block from `human_content()` if the caller set one,
and `structured_content` from `render(OutputFormat::Json)`.

---
```

- [ ] **Step 3: Update the `response` module section's `impl AgentResponse` listing**

Find:
```
    // Shared
    pub fn hint(mut self, hint: impl Into<String>) -> Self
    pub fn hints(mut self, hints: Vec<Hint>) -> Self
    pub fn recovery_hint(mut self, r: RecoveryHint) -> Self
    pub fn truncate_cells_at(mut self, limit: usize) -> Self
    /// Mark this response as an error state — reflected in `OutputFormat::Json`'s
    /// `isError` field. Beyond spec: maps directly onto MCP's `CallToolResult.isError`.
    pub fn as_error(mut self) -> Self

    pub fn render(&self, format: OutputFormat) -> String
    /// Reads the TOON slot (`items`/`fields`/`total_count`) unconditionally —
    /// not a shorthand for `render(OutputFormat::Text)`, which instead follows
    /// whichever of `.items()`/`.kv_items()` was called last. See below.
    pub fn render_toon(&self) -> String
    /// Reads the KV slot (`single_item`/`total_count`) unconditionally — the
    /// KV-path counterpart of `render_toon()`.
    pub fn render_kv(&self) -> String
    pub fn render_hints_only(&self) -> String // see supplement: three-surface seam
}
```
Change to:
```
    // Shared
    pub fn hint(mut self, hint: impl Into<String>) -> Self
    pub fn hints(mut self, hints: Vec<Hint>) -> Self
    pub fn recovery_hint(mut self, r: RecoveryHint) -> Self
    pub fn truncate_cells_at(mut self, limit: usize) -> Self
    /// Mark this response as an error state — reflected in `OutputFormat::Json`'s
    /// `isError` field. Beyond spec: maps directly onto MCP's `CallToolResult.isError`.
    pub fn as_error(mut self) -> Self
    /// Attach a human-facing companion block (`audience: user`) for MCP
    /// callers. Optional. See the `mcp` module section.
    pub fn human_content(mut self, text: impl Into<String>) -> Self

    pub fn render(&self, format: OutputFormat) -> String
    /// Reads the TOON slot (`items`/`fields`/`total_count`) unconditionally —
    /// not a shorthand for `render(OutputFormat::Text)`, which instead follows
    /// whichever of `.items()`/`.kv_items()` was called last. See below.
    pub fn render_toon(&self) -> String
    /// Reads the KV slot (`single_item`/`total_count`) unconditionally — the
    /// KV-path counterpart of `render_toon()`.
    pub fn render_kv(&self) -> String
    pub fn render_hints_only(&self) -> String // see supplement: three-surface seam
    /// Builds the MCP `CallToolResult` for this response. See the `mcp`
    /// module section for the exact shape and wire-format details.
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult
}
```

- [ ] **Step 4: Update the NAPI section's TypeScript declarations**

Find:
```typescript
/** High-level builder — mirrors the Rust `AgentResponse` API. Every mutator
 * returns `undefined`, not `this` — see "Chainable setters," above; callers
 * use sequential statements, not method chaining. */
export declare class AgentResponse {
  constructor(typeName: string);
  items(rows: ToonValue[][], fields: string[]): void;
  totalCount(n: number): void;
  kvItems(items: { key: string; value: ToonValue }[]): void;
  hint(hint: string): void;
  recoveryHint(tool: string, reason?: string): void;
  /** Marks this response as an error state, reflected in `renderJson()`'s `isError` field. */
  asError(): void;
  /** Reads the TOON slot unconditionally — see the `response` section. */
  renderToon(): string;
  /** Reads the KV slot unconditionally — see the `response` section. */
  renderKv(): string;
  /** Returns a JSON *string* — `{"body":...,"hints":[...],"recovery":[...],"isError":bool}`
   * — not a parsed value, matching the Rust side's zero-`serde_json` design. */
  renderJson(): string;
  renderHintsOnly(): string;
}
```
Change to:
```typescript
export interface Annotations {
  audience: ("assistant" | "user")[];
}

export interface ContentBlock {
  type: "text";
  text: string;
  annotations: Annotations;
}

/** The MCP `tools/call` response shape. `structuredContent` is a real parsed
 * value here (unlike `renderJson()`'s string), since it crosses the NAPI
 * boundary as a typed `#[napi(object)]` field, not hand-built JSON text. */
export interface CallToolResult {
  content: ContentBlock[];
  isError: boolean;
  structuredContent: unknown;
}

/** High-level builder — mirrors the Rust `AgentResponse` API. Every mutator
 * returns `undefined`, not `this` — see "Chainable setters," above; callers
 * use sequential statements, not method chaining. */
export declare class AgentResponse {
  constructor(typeName: string);
  items(rows: ToonValue[][], fields: string[]): void;
  totalCount(n: number): void;
  kvItems(items: { key: string; value: ToonValue }[]): void;
  hint(hint: string): void;
  recoveryHint(tool: string, reason?: string): void;
  /** Marks this response as an error state, reflected in `renderJson()`'s `isError` field. */
  asError(): void;
  /** Attaches a `user`-audience companion block, included by `toCallToolResult()`. */
  humanContent(text: string): void;
  /** Reads the TOON slot unconditionally — see the `response` section. */
  renderToon(): string;
  /** Reads the KV slot unconditionally — see the `response` section. */
  renderKv(): string;
  /** Returns a JSON *string* — `{"body":...,"hints":[...],"recovery":[...],"isError":bool}`
   * — not a parsed value; kept as hand-built text for consistency with the Rust side's
   * `render(OutputFormat::Json)`, which stays `serde_json`-free (see the `mcp` module
   * section for where this crate *does* use `serde_json`, and why). */
  renderJson(): string;
  renderHintsOnly(): string;
  /** The MCP `tools/call` response shape — see the `mcp` module section. */
  toCallToolResult(): CallToolResult;
}
```

- [ ] **Step 5: Update the duplicate "Feature flags" section**

Find:
```
## Feature flags

```toml
[features]
default = []
napi = ["dep:napi", "dep:napi-derive"]
cli  = []  # reserved: terminal-width-aware rendering (colours, wrap)
```

No async runtime dependency. No tokio, no async-std. All public functions are
sync. The `cli` feature is reserved for terminal-aware rendering (line
wrapping, colour codes for the `[DEGRADED: ...]` health signals in
`status::StatusResponse`) — out of scope for v1. michi v1 targets agent
consumers only. When the `cli` feature is fleshed out it will pull in a
terminal crate (`crossterm` for width detection and styling, or `colored` for
the minimal case) gated entirely behind the flag so the default build stays
dependency-light; see Open question Q3 for the v2 scope sketch.
```
Change to:
```
## Feature flags

```toml
[features]
default = []
napi  = ["dep:napi", "dep:napi-derive", "dep:serde_json"]
serde = ["dep:serde", "dep:serde_json"]
cli   = []  # reserved: terminal-width-aware rendering (colours, wrap)
```

No async runtime dependency. No tokio, no async-std. All public functions are
sync. `serde` is opt-in Rust-side ergonomics (`Serialize`/`Deserialize` on the
core value types, `toon::list()`) — see the Cargo.toml section above for the
full rationale. The `cli` feature is reserved for terminal-aware rendering
(line wrapping, colour codes for the `[DEGRADED: ...]` health signals in
`status::StatusResponse`) — out of scope for v1. michi v1 targets agent
consumers only. When the `cli` feature is fleshed out it will pull in a
terminal crate (`crossterm` for width detection and styling, or `colored` for
the minimal case) gated entirely behind the flag so the default build stays
dependency-light; see Open question Q3 for the v2 scope sketch.
```

- [ ] **Step 6: Fix the false publication claim**

Find:
```
## Versioning and release

- Published to [crates.io](https://crates.io) as `michi`
- npm package published to [npmjs.com](https://npmjs.com) as `michin`
```
Change to:
```
## Versioning and release

- Intended to publish to [crates.io](https://crates.io) as `michi` — not yet published
- Intended to publish the npm package to [npmjs.com](https://npmjs.com) as `michin` — not yet published
```

- [ ] **Step 7: Verify and commit**

Run: `grep -n "mcp.rs\|humanContent\|toCallToolResult\|not yet published" docs/01-spec.md`
Expected: matches for all four in the sections just edited.

Run: `cargo publish --dry-run -p michi --allow-dirty`
Expected: still succeeds (doc-only change, doesn't affect packaging).

```bash
git add docs/01-spec.md
git commit -m "docs(spec): add mcp module section, document human_content/to_call_tool_result/toCallToolResult, fix publish claim"
```

---

## Group E: Peripheral docs

### Task 9: Fix `docs/00-overview.md`'s stale npm name and fictional TypeScript quick-start

**Files:**
- Modify: `docs/00-overview.md`

- [ ] **Step 1: Fix the npm wrapper name**

Find:
```
The crate is intentionally narrow: **no protocol knowledge, no async runtime, no CLI framework**.
Pure computation — data in, strings and types out. TypeScript consumers reach it via the NAPI npm
wrapper `michi`; Rust consumers take a direct crates.io or git dependency.
```
Change to:
```
The crate is intentionally narrow: **no protocol knowledge, no async runtime, no CLI framework**.
Pure computation — data in, strings and types out. TypeScript consumers reach it via the NAPI npm
wrapper `michin`; Rust consumers take a direct crates.io or git dependency.
```

- [ ] **Step 2: Replace the fictional chainable TypeScript quick-start**

Find:
```typescript
import { toon } from "michi";

const issues = [
  { number: 51815, title: "[Bug]: Telegram plugin", state: "open" },
  { number: 51812, title: "dark mode request", state: "open" },
];

// Identical primitives, via the NAPI wrapper.
const response = toon
  .list("issues", issues)
  .total(8771)
  .hint("gh-axi issue view <number>", "view an issue")
  .render();

process.stdout.write(response);
```
Change to:
```typescript
import { AgentResponse } from "michin";

const issues = [
  { number: 51815, title: "[Bug]: Telegram plugin", state: "open" },
  { number: 51812, title: "dark mode request", state: "open" },
];

// No toon.list() equivalent crosses the NAPI boundary — that convenience is
// Rust/serde-feature-only. Build tagged cell values by hand and use the
// non-chainable AgentResponse API (every mutator returns void, not `this`).
const rows = issues.map((i) => [
  { type: "int", intVal: i.number },
  { type: "str", strVal: i.title },
  { type: "str", strVal: i.state },
]);

const r = new AgentResponse("issues");
r.items(rows, ["number", "title", "state"]);
r.totalCount(8771);
r.hint("Run `gh-axi issue view <number>` to view an issue");

process.stdout.write(r.renderToon());
```

- [ ] **Step 3: Verify and commit**

Run: `grep -n 'from "michi"\|toon\s*$\|\.total(\|\.render();' docs/00-overview.md`
Expected: no matches (fictional API removed; npm import now says `michin`).

```bash
git add docs/00-overview.md
git commit -m "docs(overview): fix stale npm package name, replace fictional chainable TS API with the real AgentResponse flow"
```

---

## Final verification

- [ ] Run the complete workspace check across all feature combinations:
  ```bash
  cargo build -p michi
  cargo build -p michi --features serde
  cargo build -p michi --features napi
  cargo build -p michi --all-features
  cargo nextest run -p michi --all-features
  cargo clippy -p michi --all-features -- -D warnings
  cargo fmt -p michi -- --check
  ```
  Expected: all clean. (The pre-existing, unrelated `pipeline::verify_finding::*` test
  failures from the stray uncommitted scratch file in `src/pipeline/mod.rs`, if still
  present, are not this plan's concern — confirm via `git status src/pipeline/mod.rs` and
  note it in the final report rather than fixing or being surprised by it.)
- [ ] Run `cd packages/michi-node && pnpm build --platform && pnpm test` — confirm the NAPI
  boundary works end-to-end including the new `type`/`annotations.audience` shape and
  `humanContent()`.
- [ ] Run `cargo publish --dry-run -p michi --allow-dirty` — confirm the crate still packages
  cleanly.
- [ ] Re-run the exact literal conformance check from the validation pass: compile and run a
  throwaway snippet (or reuse `mcp::tests::call_tool_result_wire_json_matches_mcp_shape`) and
  confirm the JSON contains `"type":"text"`, `"annotations":{"audience":[...]}`, `"isError"`,
  `"structuredContent"` as a real embedded object — and contains no `is_error` or
  `structured_content` (snake_case) anywhere.
- [ ] Grep for any remaining stale claims the validation flagged:
  `grep -n "assembled by the calling MCP framework\|deliberately absent\|zero-.serde_json. rationale\|Published to \[crates.io\]" docs/01-spec.md`
  (expect no output).
- [ ] Confirm `packages/michi-node/README.md` now lists both `humanContent` and
  `toCallToolResult`: `grep -n "humanContent\|toCallToolResult" packages/michi-node/README.md`.
