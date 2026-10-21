# Security Policy

## Reporting a Vulnerability

Email **security@orin-dx.com** — don't open a public issue for anything exploitable before a fix ships.

Include:
- Which crate or package is affected (`michi-truncate`, `michi-resilience`, `michi-toon`, `michi-core`, `michi-pipeline`, `michi`, or `@orin-dx/michi`)
- The concrete failure scenario and how to trigger it
- Steps to reproduce, if you have them

Expect an acknowledgment within 5 business days. We'll keep you posted as a fix moves through triage, and credit you in the release notes unless you'd rather stay anonymous.

## Scope

`michi` formats and parses data that is often untrusted or attacker-influenced by the time it reaches this library — that's the actual job (turning arbitrary tool/agent output into TOON text, and parsing TOON back). The relevant threat model:

- **Panics on malformed input** — the workspace's own rule is zero panics in library code (`unwrap()`/`expect()`/`panic!` are avoided outside tests). A crafted input that panics `michi-toon`'s renderer or parser is a real finding — in a server or CLI embedding this library, a panic is a crash, and a crash on attacker-controlled input is a denial-of-service vector.
- **UTF-8 truncation boundary bugs** — `michi-truncate`'s whole purpose is safe truncation on Unicode scalar boundaries (`floor_char_boundary`). A string that produces corrupted output, an out-of-bounds slice, or a panic instead of a clean truncation is in scope.
- **The `unsafe` boundary** — `unsafe` code is restricted to `src/napi.rs` (`#![deny(unsafe_code)]` everywhere else in the workspace). Any memory-safety issue at the Rust/Node.js FFI boundary — a use-after-free, buffer overrun, or lifetime violation across `@orin-dx/michi`'s native bindings — is high-severity and in scope. So is any `unsafe` block that shows up anywhere it isn't supposed to.
- **Resilience/retry logic** — `michi-resilience`'s back-off math and `Retry-After` parsing feeding into `michi-pipeline`'s circuit breaker. A malformed `Retry-After` header or edge-case duration that defeats the back-off (e.g., produces a zero or negative delay, or an unbounded one) is in scope.
- **Supply chain** — this is a mixed Rust/Node workspace; both `Cargo.lock` and the npm dependency tree (`packages/michi-node`, `oxfmt` at the repo root) are relevant attack surface.

Out of scope: issues in consuming applications that misuse the API in ways the type system doesn't prevent, or in `tokio`/`napi`/other upstream dependencies themselves — report those upstream.

## Supported Versions

`michi` is not yet published to crates.io or npm (see the README). Until a first tagged release ships, security fixes land on `main` only — there is no older version to backport to.
