# michi-node

## 0.3.0

- Versions in lockstep with the `michi-core` and `michi-pipeline` crates (0.3.0); the exports now come from `michi-core`'s `napi` feature.
- Every rendered string is TOON 4.4: empty lists render `key: []`, null cells `null`, empty strings `""`, help blocks inline (`help[N]: a,b`), KV/error/status/recovery blocks as TOON, and no output ends with a newline. See `crates/michi-core/CHANGELOG.md` for the full list.
- `renderToon`/`AgentResponse` quote, escape and canonicalize values per the spec; integers above `Number.MAX_SAFE_INTEGER` are not affected (inputs are validated JS safe integers).
- An `AgentResponse` whose rows fail validation now reports `isError: true` in `toCallToolResult()` and `renderJson()`.
- Truncated cells say how many characters were cut and no longer suggest `full=true`; pass the new `JsToonOptions.truncationHint` / `AgentResponse.truncationHint()` to add your own instruction.
- `renderKv` throws `kv_validation_failed: …` for duplicate or reserved keys (`totalCount`, `help`, `recovery`).

## 0.2.0

- Every numeric argument crossing the NAPI boundary is now validated through a dedicated newtype (`JsCount`, `JsInt`, `JsRanged`/`JsDecimals`/`JsRetryCount`/`JsHttpStatus`, `JsFloat`, `JsUnitInterval`, `JsDelayMillis`) instead of ad hoc per-call-site coercion. Generated TypeScript signatures are unchanged (still plain `number`), but out-of-domain values that were previously silently coerced now throw:

  - `truncate`'s `maxChars` and `JsToonOptions`/`JsAgentResponse`'s `totalCount`: a negative or fractional value was previously clamped to `0` (or, for `JsToonOptions.totalCount`, silently dropped); now rejected with a thrown error.
  - `JsToonValue.decimalsVal`: a value outside `[0, 20]` was previously clamped into range; now rejected.
  - `nextRetryDelay`'s `maxRetries`/`attempt`, `baseDelayMs`/`maxDelayMs`, `jitterFactor`/`jitterSeed`, and `retryAfterMs`: all seven parameters now go through the same validating newtypes and reject non-finite, negative, fractional (where integral), or overflow-inducing values with a consistent error message, instead of the previous ad hoc validation that only covered a subset of these parameters.
  - `isRetryableStatus`'s `status`: a value outside `[100, 599]` was previously coerced to `0` via a fallible cast with a silent fallback; now rejected.
  - `JsToonValue.intVal` widened from a 32-bit range to the full JS-safe-integer range (`±9007199254740991`), so large integers (for example millisecond timestamps) round-trip losslessly through `renderToon`/`renderKv` instead of being truncated to 32 bits.

- NAPI `renderToon()` rejections are now prefixed `toon_validation_failed: ` (e.g. `toon_validation_failed: row 0 has 1 values but 2 fields declared`). Previously the rejection reason was the bare validation message. Any caller matching on exact error text needs to update for the new prefix.
- `renderToon`, `renderKv`, `renderStatus`, and `JsAgentResponse#items`/`#kvItems` now reject a `JsToonValue` whose declared `type` doesn't match its payload, instead of silently defaulting. Previously, `{ type: 'int' }` with `intVal` omitted rendered as `0`, and an unrecognized `type` (e.g. a typo like `'integer'`) silently rendered as `null`/dropped the value. Both cases now throw a descriptive error (e.g. `JsToonValue: type is "int" but intVal is missing`, or `JsToonValue: unknown type "integer": expected "str", "int", "float", "bool", or "null"`) before any rendering happens.
