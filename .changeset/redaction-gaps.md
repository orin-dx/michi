---
michi-core: minor
michi-pipeline: patch
---

`redact_common_shapes`, and so `DomainError::from_error` and the pipeline error conversion, now redacts values it used to leave: the credential after a scheme word (`Authorization: Basic ...`, `Bearer`, `Token`, `Digest`), quoted values (`token="x"`, `'x'`, JSON `"password": "x"`, JSON-escaped `\"x\"`), `Key: value` headers (`X-Api-Key:`, `Cookie:`, `Set-Cookie:`), and values after the fullwidth `＝` and `：`. Text that has `token: ...` or similar in prose is redacted too, which is the safe direction. A long line of `key="` pairs no longer takes quadratic time.

`redact_secrets` copies an existing `[REDACTED]` through, so a secret such as `RED` no longer turns the marker into `[[REDACTED]ACTED]` on a second pass; redacting twice is a no-op unless a secret contains `[`.

Step text in a converted `ExecutionError` is now at most 512 bytes including the `…` marker (it was 515).
