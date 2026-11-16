---
michi-core: minor
michi-pipeline: minor
---

Breaking: converting an `ExecutionError` to a `DomainError` no longer puts step text in the trusted `message`. The message is now fixed text (`http 502`, `the step reported an error`, `step panicked`, `deadline passed`), and what the step or server said (an HTTP body, a `failed()` message, a panic payload) is one entry in `causes`, redacted and cut to 512 bytes including the `…` marker. Read it from `causes`, not `message`.

`DomainError::from_error` and `from_error_with_causes` now use the new `michi_core::redact::redact_common_shapes`. On top of URL userinfo it redacts the value of `token=`, `access_token=`, `api_key=`, `password=` and similar pairs (quoted, `'x'`, JSON `"password": "x"` or JSON-escaped `\"x\"`, and after the fullwidth `＝` and `：`), `Key: value` headers (`X-Api-Key:`, `Cookie:`, `Set-Cookie:`), the credential after a scheme word (`Authorization: Basic ...`, `Bearer`, `Token`, `Digest`), and `ghp_`, `github_pat_`, `npm_` and `pypi-` tokens. Text with `token: ...` or similar in prose is redacted too, which is the safe direction. The scan is linear in the text, also for a long line of `key="` pairs and for a long text of URLs (`redact_url_userinfo` no longer rescans the rest of the text once per URL, which made a 1 MiB body of URLs take minutes). `redact_url_userinfo` also handles JSON-escaped (`https:\/\/`) and percent-encoded (`https%3A%2F%2F`) URLs and a `"` inside a password. This is best effort: the docs list what is and is not covered.

`redact_secrets` copies an existing `[REDACTED]` through, so a secret such as `RED` no longer turns the marker into `[[REDACTED]ACTED]` on a second pass; redacting twice is a no-op unless a secret contains `[`.
