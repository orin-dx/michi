# Security Policy

## Reporting a Vulnerability

Email **security@orin-dx.com** — don't open a public issue for anything exploitable before a fix ships.

Include:

- Which crate or package is affected (`michi-core`, `michi-pipeline`, `michi-graph`, or `@orin-dx/michi`), and for `michi-core`, which module (e.g. `toon`, `truncate`, `resilience`, `napi`)
- The concrete failure scenario and how to trigger it
- Steps to reproduce, if you have them

Expect an acknowledgment within 5 business days. We'll keep you posted as a fix moves through triage, and credit you in the release notes unless you'd rather stay anonymous.

## Scope

`michi` formats data that is often untrusted or attacker-influenced by the time it reaches this library — that's the actual job (turning arbitrary tool/agent output into TOON text; michi has no public TOON parser). The relevant threat model:

- **Panics on malformed input** — the workspace's own rule is zero panics in library code (`unwrap()`/`expect()`/`panic!` are avoided outside tests). A crafted input that panics `michi_core::toon`'s renderer (or `michi-graph`'s validation) is a real finding — in a server or CLI embedding this library, a panic is a crash, and a crash on attacker-controlled input is a denial-of-service vector.
- **UTF-8 truncation boundary bugs** — `michi_core::truncate`'s whole purpose is safe truncation on Unicode scalar boundaries (`floor_char_boundary`). A string that produces corrupted output, an out-of-bounds slice, or a panic instead of a clean truncation is in scope.
- **The `unsafe` boundary** — `unsafe` code is restricted to michi-core's `napi` module (`crates/michi-core/src/napi.rs` and `napi/`; `unsafe_code = "deny"` everywhere else in the workspace). Any memory-safety issue at the Rust/Node.js FFI boundary — a use-after-free, buffer overrun, or lifetime violation across `@orin-dx/michi`'s native bindings — is high-severity and in scope. So is any `unsafe` block that shows up anywhere it isn't supposed to.
- **Resilience/retry logic** — `michi_core::resilience`'s back-off math and `Retry-After` parsing feeding into `michi-pipeline`'s circuit breaker. A malformed `Retry-After` header or edge-case duration that defeats the back-off (e.g., produces a zero or negative delay, or an unbounded one) is in scope.
- **Supply chain** — this is a mixed Rust/Node workspace; both `Cargo.lock` and the npm dependency tree (`packages/michi-node`, `oxfmt` at the repo root) are relevant attack surface.

What michi does and does not promise about untrusted text is in the [trust model](docs/spec/02-toon-format.md#trust-model): one value cannot create extra lines, fields or cells, but michi cannot stop prompt injection in data.

Out of scope: issues in consuming applications that misuse the API in ways the type system doesn't prevent, or in `tokio`/`napi`/other upstream dependencies themselves — report those upstream.

## Supported Versions

`michi-core`, `michi-pipeline`, `michi-graph` and `@orin-dx/michi` release together on one version.

| Version             | Supported |
| ------------------- | --------- |
| Latest release      | Yes       |
| Any earlier release | No        |

Security fixes ship in a new release of all four packages. Before 1.0, fixes are not backported to earlier minor versions; upgrade to the latest. Versions before 0.3.1 were never published.
