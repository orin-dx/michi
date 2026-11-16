---
michi-core: minor
michi-pipeline: minor
---

Breaking: converting an `ExecutionError` to a `DomainError` no longer puts step text in the trusted `message`. The message is now fixed text (`http 502`, `the step reported an error`, `step panicked`, `deadline passed`), and what the step or server said (an HTTP body, a `failed()` message, a panic payload) is one entry in `causes`, redacted and cut to 512 bytes. Read it from `causes`, not `message`.

`DomainError::from_error` and `from_error_with_causes` now use the new `michi_core::redact::redact_common_shapes`. On top of URL userinfo it redacts the value of `token=`, `access_token=`, `api_key=`, `password=` and similar pairs, and `ghp_`, `github_pat_`, `npm_`, `pypi-` and `Bearer` tokens. `redact_url_userinfo` also handles JSON-escaped (`https:\/\/`) and percent-encoded (`https%3A%2F%2F`) URLs and a `"` inside a password. This is best effort: the docs now list what is and is not covered.
