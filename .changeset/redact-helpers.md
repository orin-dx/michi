---
michi-core: minor
---

New `michi_core::redact` module: `redact_secrets(text, &secrets)` replaces known secret values with `[REDACTED]`, and `redact_url_userinfo(text)` removes the `user:password@` part of URLs in a text (a password containing `@` or `/` is removed whole). Both are best-effort and do not find secrets in general. `DomainError::from_error` and `from_error_with_causes` now apply `redact_url_userinfo` to the error text they copy, so a URL with credentials in a library error no longer reaches the agent; an `@` later in a URL's path makes the redaction over-reach rather than leak.
