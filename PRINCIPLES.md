# Principles

Two questions come up over and over: **should this go in michi?** and **how should this work get done?** This doc answers both, so nobody — human or agent — has to re-derive the reasoning from scratch each time. It's the "why" behind `CONTRIBUTING.md` and `CLAUDE.md`'s rules, not a replacement for either.

---

## What belongs in michi

A primitive earns a place here only if it clears all five gates below. These are yes/no checks with evidence, not vibes — if you can't point to the evidence, it doesn't clear the gate yet.

### 1. Rendering, not domain logic

It turns already-decided data into agent-readable strings/types. It doesn't decide _what_ data survives.

- One established exception: char/byte-level truncation (`truncate`, `ToonOptions::max_cell_len`) — treated as rendering safety, not business logic.
- **The ceiling:** if a tool's real need is more sophisticated than a generic primitive can serve — a custom ranking algorithm, a heuristic tuned to one tool's data shape — that's a sign the primitive doesn't belong in michi, not a sign to make michi's primitive more complex. See [Split the decision from the signal](#split-the-decision-from-the-signal) for how to resolve this without compromising on either.

### 2. Small default dependency set

The default build depends only on `thiserror` and `compact_str`. Anything else is opt-in behind a Cargo feature.

- Adding a mandatory dependency needs a recorded decision in `docs/spec/06-decisions.md`. Prefer caller-supplied injection (a closure, a trait) first.

### 3. Genuinely cross-tool — verified, not assumed

Earns inclusion when independent, real tools would otherwise hand-roll the identical pattern.

- "Independent" and "real" are load-bearing. One tool using a pattern heavily is one data point, not proof.
- **What clearing this bar looks like:** a survey across several real agent-facing dev tools (a code-search engine, a CLI proxy, a context-compression tool, a code-graph server) found `help[]`-shaped "note for the agent" hints independently reinvented, inconsistently, in every one of them.
- A pattern seen in only one tool — however heavily repeated inside that tool — isn't there yet. Watch for a second independent confirmation before building it.

### 4. Doesn't duplicate an existing primitive

Check `hints` / `recovery` / `idempotency` / `status` / `error` / `mcp` first. Verify the distinction concretely — don't assume novelty from a vibe.

- "This looks similar to X" means go read X's source, not assume it's covered.
- Example: a proposed "diagnostics" primitive looked adjacent to `idempotency::PartialSuccess` — until actually reading `PartialSuccess`'s doc comment showed it's scoped to multi-step _mutating_ operations, not read-path degradation. They turned out genuinely distinct, but that took checking, not assuming either way.

### 5. Deviations get tracked, not silently absorbed

Where the final shape diverges from the original ask or an earlier draft, it goes in a Design Decisions table. Spec and code should never quietly drift apart.

---

## Split the decision from the signal

Gate 1's ceiling ("stay generic" vs. "real tools need real sophistication") isn't a tension to compromise on — it's usually an architecture problem. Separate the _domain-specific decision_ from the _universal signal_.

**Worked example: truncation.** Deciding _which_ items survive a budget is tool-specific — personalized-PageRank over a call graph, a statistical outlier-preserving sampler, a simple first-N cutoff. None of that belongs in michi. But _reporting_ what got cut, in a consistent agent-readable shape, is exactly what every tool needs regardless of how it decided:

```rust
/// What survived some truncation/budget decision, however it was made.
pub struct TruncationOutcome<T> {
    pub kept: Vec<T>,
    pub total_before: usize,
    pub truncated: bool,
}

impl<T> TruncationOutcome<T> {
    /// Michi's standard help[]-shaped signal for what got cut — same
    /// contract regardless of which strategy produced this outcome.
    pub fn hint(&self, guidance: impl Into<String>) -> Option<Hint> { /* ... */ }
}

// "Best practice" strategies michi ships, with no extra dependencies, for the common case:
pub fn truncate_by_count<T>(items: Vec<T>, max: usize) -> TruncationOutcome<T>;
pub fn truncate_by_bytes<T>(items: Vec<T>, max_bytes: usize, size_of: impl Fn(&T) -> usize) -> TruncationOutcome<T>;
```

A caller with a genuinely novel heuristic never has to fit their algorithm into michi's shape — they wrap whatever they decided in `TruncationOutcome` and get the same hint/report format everyone else gets. Michi owns the contract, not the heuristic.

**When a new primitive seems too domain-specific to generalize, ask whether it splits this way first.** Often only half of it actually is.

---

## How work gets done

Heuristics, not hard rules — process needs contextual judgment the content checklist doesn't.

### 1. TDD, always

Failing test first, verified red for the right reason, then implementation, then green. No implementation code without a preceding red test.

### 2. Independent verification before "done"

A separate pass re-checks against the spec by running the actual output — not by trusting the implementer's self-report or re-reading the diff.

- Per-task review alone has missed real bugs that a separately-scoped validation pass, checking actual wire output, caught.
- "Every task's reviewer said pass" and "the feature actually works" are different questions. Both need answering.

### 3. Verify before asserting

Claims about external state — a registry's current contents, another project's actual spec, a dependency's real behavior — get checked directly, not recalled from memory or assumed from a plausible-sounding claim.

- Can't quickly re-verify something? Say so rather than asserting confidence.

### 4. History is append-only

Superseded plans/specs get a notice pointing at what replaced them, not a rewrite pretending they always said the current thing.

- A plan document that was correct when written stays as written. Corrections happen in new commits, not retroactive edits to old ones.

### 5. Publish/release gates are earned

Passing a component's own tests isn't "ready to ship."

- A release gate (e.g. crates.io publish conditioned on a real consumer integration, not just internal completeness) exists for a reason.
- Treat it as binding until the condition it names is actually met — not as a formality to route around once everything else looks finished.

### 6. Invariant enforcement policy

Three patterns apply at different layers. Pick the right one for the layer — don't invent a fourth.

- **Infallible constructors normalize.** `new()` clamps inputs to valid ranges silently (e.g. `jitter_factor` to `[0.0, 1.0]`, `base_delay` to `≤ max_delay`). Never `unwrap()`/`expect()` in lib code. Expose a `try_new()` or `validate()` alongside for callers who want explicit error signals — but these are additive, they don't replace the normalizing constructor.
- **Rendering never fails silently, and never reports a failure as success.** Two cases:
  - Input that can always be made safe is sanitized at the render layer, and that `render*()` returns `String`: escape or replace structural characters so one value cannot create extra lines, fields or cells.
  - Input whose shape can be wrong (TOON row length against its fields, a structural character in a header name) is validated, and the free functions return `Result` (`render_toon`, `ToonDocument::validate`, `toon::list`). There is no `debug_assert!` fallback: debug and release builds behave the same.
  - A builder that must always produce a response (`AgentResponse`) renders a validation failure as an error block and marks the response as an error on every surface: the text body, the JSON `isError`, and MCP `isError`. Callers can check `AgentResponse::is_error()` before sending.
- **Wire it in or document it as opt-in.** A `validate()` or `try_new()` that has no caller is documentation masquerading as validation. Any new validation method must either be called from at least one mandatory code path (not just tests) or carry an explicit `/// Caller opt-in — not called automatically` doc comment.
- **Empty strings are accepted everywhere.** michi doesn't validate business rules — constructors accept empty strings unconditionally. The render result for an empty field is an empty field, not an error. This is a deliberate contract, not an oversight; document it if adding a new constructor where it might surprise a reader.

---

## Using this document

- **Content questions** ("should X be a michi primitive?") → the five gates above, in order. Stop at the first "no."
- **Process questions** ("is this ready to ship / merge / call done?") → the five heuristics, weighed together, not mechanically checked off.
- **When they pull in different directions** — a genuinely useful pattern seen only once, a process shortcut that would ship something faster — that's not a bug in the framework. It's the signal to slow down and think it through explicitly, rather than pick the convenient answer.
