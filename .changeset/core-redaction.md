---
michi-core: minor
---

Error text that reaches an agent now has more credentials removed. `DomainError::from_error` and `from_error_with_causes` use the new `michi_core::redact::redact_common_shapes`, which, on top of URL userinfo, redacts:

- the value of `token=`, `access_token=`, `api_key=`, `password=` and similar pairs, quoted (`'x'`), as JSON (`"password": "x"` or JSON-escaped `\"x\"`), and after the fullwidth `＝` and `：`
- `Key: value` headers such as `X-Api-Key:`, `Cookie:` and `Set-Cookie:`
- the credential after a scheme word (`Authorization: Basic ...`, `Bearer`, `Token`, `Digest`)
- `ghp_`, `github_pat_`, `npm_` and `pypi-` tokens

Details:

- Prose such as `token: ...` is redacted too, which is the safe direction.
- `redact_url_userinfo` also handles JSON-escaped (`https:\/\/`) and percent-encoded (`https%3A%2F%2F`) URLs, and a `"` inside a password.
- `redact_secrets` is now idempotent: it copies an existing `[REDACTED]` through, so a secret such as `RED` no longer turns the marker into `[[REDACTED]ACTED]` on a second pass. A known secret that starts with or contains the marker (`[REDACTED]private-tail`) is still removed whole; redacting twice is a no-op unless a secret contains `[`.
- This is best effort: the docs list what is and is not covered.
