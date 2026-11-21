//! NAPI export surface for the `@orin-dx/michi` npm package (`packages/michi-node`).
//!
//! `packages/michi-node/src/lib.rs` re-exports this module (`pub use michi_core::napi::*;`) rather than defining
//! `#[napi]` items directly, so the export logic can be unit-tested here alongside the rest of the crate.
//! `packages/michi-node` remains a separate Cargo crate only because `crate-type = ["cdylib"]` cannot coexist with a
//! regular `[lib]` in one `Cargo.toml` (see "NAPI" in `docs/spec/04-mcp-and-napi.md`).
//!
//! The exports live in submodules by concern, and nothing but `num` is public: the `#[napi]` items register
//! themselves with the addon, and no Rust caller needs them by name. Each export converts its untyped JS arguments
//! through `input` (which bounds them) and then calls a private function that renders; the limits live in `input`
//! only.
//!
//! | Module | Exports |
//! | --- | --- |
//! | `toon` | `renderToon`, `renderKv` |
//! | `hints` | `renderHints`, `appendHints`, `renderRecovery` |
//! | `outcome` | `renderGithubAnnotation`, and the outcome types `AgentResponse` takes |
//! | `resilience` | `parseRetryAfter`, `nextRetryDelay`, `isRetryableStatus`, `truncate` |
//! | `response` | the `AgentResponse` class and `toCallToolResult` |
//! | `value` | the `ToonValue` cell and `KvItem`, shared by the rest |
//!
//! Every export uses `#[napi(catch_unwind)]` so a Rust panic cannot crash the host Node.js process, and every export
//! that accepts caller-controlled collection sizes validates them against hard limits before doing any work, since a
//! single crafted call from untrusted JS is otherwise able to force unbounded synchronous allocation on Node's
//! single-threaded event loop. The byte cap also bounds the output: a render is at most 19 times the bytes held plus
//! 4 KiB, because every held byte is charged for each place a writer can put it (see `input::MAX_INPUT_BYTES` and
//! `cost`).
//!
//! This is the one module in the crate exempted from the workspace's
//! `unsafe_code = "deny"` lint (`[workspace.lints.rust]` in the root
//! `Cargo.toml`): `#[napi(catch_unwind)]` expands to macro-generated FFI glue
//! that needs `unsafe` to call into the N-API C ABI. No unsafe is hand-written
//! in this file; the only hand-written `unsafe` is in the `num` submodule's
//! `FromNapiValue`/`ToNapiValue` impls, which those napi traits require.
#![allow(unsafe_code)]
// napi-derive registers the exports only outside test builds, so in a test build they look unused.
#![cfg_attr(test, allow(dead_code))]
#![deny(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

/// Validating newtypes for the NAPI numeric boundary (shared kernel for
/// both the rendering and resilience domains).
pub mod num;

/// What a held item can cost in output beyond its own bytes: the names a `truncated` or `totalCount` row writes again.
mod cost;
/// Bounded, incremental conversion of the exports' collection and string arguments.
mod input;

mod hints;
mod outcome;
mod resilience;
mod response;
mod toon;
mod value;

#[cfg(test)]
mod tests {
    use super::input::{MAX_FIELDS, MAX_HINTS, MAX_ROWS, MAX_STEPS};

    /// Every source file of the module tree, for the scans below.
    const SOURCES: [(&str, &str); 9] = [
        ("napi.rs", include_str!("napi.rs")),
        ("napi/cost.rs", include_str!("napi/cost.rs")),
        ("napi/hints.rs", include_str!("napi/hints.rs")),
        ("napi/input.rs", include_str!("napi/input.rs")),
        ("napi/outcome.rs", include_str!("napi/outcome.rs")),
        ("napi/resilience.rs", include_str!("napi/resilience.rs")),
        ("napi/response.rs", include_str!("napi/response.rs")),
        ("napi/toon.rs", include_str!("napi/toon.rs")),
        ("napi/value.rs", include_str!("napi/value.rs")),
    ];

    // --- the hard-cap constants are pinned to their documented values. ---

    #[test]
    fn cap_constants_are_pinned_to_documented_values() {
        assert_eq!(MAX_ROWS, 100_000);
        assert_eq!(MAX_FIELDS, 1_000);
        assert_eq!(MAX_HINTS, 10_000);
        assert_eq!(MAX_STEPS, 10_000);
        assert_eq!(super::input::MAX_INPUT_BYTES, 16 * 1024 * 1024);
        assert_eq!(super::input::MAX_OUTPUT_FACTOR, 19);
        assert_eq!(super::input::OUTPUT_ENVELOPE, 4096);
    }

    // The output bound charges a cell for the names a `truncated` row writes again only if the cell is cut, and a cell
    // is cut at the response's default cap. A call that changed the cap would cut more cells than the charge covers.
    #[test]
    fn no_export_changes_the_per_cell_cap() {
        let setters = [["truncate_cells", "_at"].concat(), [".max_cell", "_len("].concat()];
        for (name, src) in SOURCES {
            for setter in &setters {
                // The scan's own tokens are built above, so this file does not contain them.
                assert!(!src.contains(setter.as_str()), "{name} sets the cell cap with `{setter}`");
            }
        }
    }

    // --- AC-032: packages/michi-node/src/lib.rs contains only a doc-comment
    // header and `pub use michi_core::napi::*;` — no other public items. ---

    #[test]
    fn ac032_michi_node_lib_rs_contains_only_the_reexport() {
        let Some(src) = crate::test_support::workspace_file("packages/michi-node/src/lib.rs") else { return };
        let non_comment_non_blank: Vec<&str> =
            src.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("//")).collect();
        assert_eq!(
            non_comment_non_blank,
            vec!["pub use michi_core::napi::*;"],
            "packages/michi-node/src/lib.rs must contain only a doc-comment header and \
             `pub use michi_core::napi::*;`, got non-comment/non-blank lines: {non_comment_non_blank:?}"
        );
    }

    /// Returns `true` if `line` contains the token `unsafe` used as a block
    /// or item qualifier — i.e. immediately (modulo whitespace) followed by
    /// `{`, `fn`, `impl`, or `trait` — approximating the regex
    /// `\bunsafe\s*(\{|fn|impl|trait)` from AC-033a without a regex
    /// dependency. Deliberately does not match `unsafe_code` (the attribute
    /// token used by `#![deny(unsafe_code)]` / `#![allow(unsafe_code)]`,
    /// covered separately by AC-033b) because the character immediately
    /// following `unsafe` there is `_`, which fails the word-boundary check.
    fn line_has_unsafe_qualifier(line: &str) -> bool {
        let bytes = line.as_bytes();
        let mut i = 0;
        while let Some(rel) = line[i..].find("unsafe") {
            let start = i + rel;
            let before_ok = start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
            let after = start + "unsafe".len();
            let after_ok = after >= bytes.len() || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
            if before_ok && after_ok {
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let rest = &line[j..];
                if rest.starts_with('{')
                    || rest.starts_with("fn")
                    || rest.starts_with("impl")
                    || rest.starts_with("trait")
                {
                    return true;
                }
            }
            i = after;
            if i > line.len() {
                break;
            }
        }
        false
    }

    // --- AC-033a: the only unsafe blocks/fns in the napi module tree are
    // FromNapiValue/ToNapiValue trait impl bodies in src/napi/num.rs; no
    // hand-written unsafe exists in src/lib.rs, src/napi.rs, or
    // packages/michi-node/src/lib.rs. ---

    #[test]
    fn ac033a_unsafe_confined_to_napi_num_ffi_trait_impls() {
        let lib_src = include_str!("lib.rs");
        let node_src = crate::test_support::workspace_file("packages/michi-node/src/lib.rs");
        let num_src = include_str!("napi/num.rs");

        let mut sources = vec![("src/lib.rs", lib_src)];
        sources.extend(SOURCES);
        if let Some(node_src) = node_src.as_deref() {
            sources.push(("packages/michi-node/src/lib.rs", node_src));
        }
        for (name, src) in sources {
            let hits: Vec<&str> = src.lines().filter(|l| line_has_unsafe_qualifier(l)).collect();
            assert!(hits.is_empty(), "{name} must contain no hand-written `unsafe` qualifiers, found: {hits:?}");
        }

        let lines: Vec<&str> = num_src.lines().collect();
        let impl_line_indices: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("impl") && (l.contains("FromNapiValue") || l.contains("ToNapiValue")))
            .map(|(i, _)| i)
            .collect();
        assert!(!impl_line_indices.is_empty(), "expected at least one FromNapiValue/ToNapiValue impl in num.rs");

        let mut unsafe_hit_count = 0;
        for (i, line) in lines.iter().enumerate() {
            if line_has_unsafe_qualifier(line) {
                unsafe_hit_count += 1;
                let near = impl_line_indices.iter().any(|&ii| i >= ii && i <= ii + 3);
                assert!(
                    near,
                    "num.rs:{}: `{}` is not within 3 lines of a FromNapiValue/ToNapiValue impl line",
                    i + 1,
                    line.trim()
                );
            }
        }
        assert!(unsafe_hit_count > 0, "expected num.rs to contain the sanctioned FFI-boundary unsafe occurrences");
    }

    // --- AC-033b: the workspace lints deny unsafe_code and this crate opts in;
    // src/napi.rs has #![allow(unsafe_code)] immediately preceded by an
    // explanatory comment. ---

    #[test]
    fn ac033b_deny_and_allow_unsafe_code_attributes_present() {
        // unsafe_code = "deny" lives in [workspace.lints.rust] (Cargo.toml),
        // not a #![deny(unsafe_code)] attribute in lib.rs -- see Cargo.toml's
        // comment on why only unsafe_code is centralized this way.
        // Read from the workspace: the packaged crate has no root manifest and inlines the workspace lints.
        if let (Some(workspace_toml), Some(cargo_toml)) = (
            crate::test_support::workspace_file("Cargo.toml"),
            crate::test_support::workspace_file("crates/michi-core/Cargo.toml"),
        ) {
            assert!(
                workspace_toml.contains("unsafe_code = \"deny\""),
                "Cargo.toml's [workspace.lints.rust] must set unsafe_code = \"deny\""
            );
            assert!(
                cargo_toml.contains("[lints]\nworkspace = true"),
                "Cargo.toml's [package] must opt into workspace lints via [lints]\\nworkspace = true"
            );
        }

        let napi_src = include_str!("napi.rs");
        let idx = napi_src.find("\n#![allow(unsafe_code)]").expect("src/napi.rs must contain #![allow(unsafe_code)]");
        let preceding = &napi_src[..idx];
        let last_non_blank = preceding.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        assert!(
            last_non_blank.trim_start().starts_with("//"),
            "expected a comment immediately preceding #![allow(unsafe_code)], got: {last_non_blank:?}"
        );
        // Check the comment block explains the reason (catch_unwind macro expansion),
        // not just that some comment happens to be adjacent.
        let block: String = preceding
            .lines()
            .rev()
            .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            block.contains("catch_unwind"),
            "comment preceding #![allow(unsafe_code)] must explain the exemption via catch_unwind, got: {block:?}"
        );
    }

    #[test]
    fn ac034a_every_napi_attributed_fn_has_catch_unwind() {
        let mut checked = 0;
        for (name, src) in SOURCES {
            let lines: Vec<&str> = src.lines().map(str::trim).collect();
            for (i, line) in lines.iter().enumerate() {
                if !line.starts_with("pub fn") {
                    continue;
                }
                // The attribute and doc lines directly above the fn, including a multi-line `#[napi(...)]`
                // (its inner lines are read bottom-up from the closing `)]` to the opening `#[`).
                let mut attrs: Vec<&str> = Vec::new();
                let mut j = i;
                let mut open = false;
                while j > 0 {
                    let prev = lines[j - 1];
                    if open || prev.starts_with("#[") || prev.starts_with(")]") || prev.starts_with("///") {
                        open = (open || prev.starts_with(")]")) && !prev.starts_with("#[");
                        attrs.push(prev);
                        j -= 1;
                    } else {
                        break;
                    }
                }
                let attrs = attrs.join("\n");
                if attrs.contains("#[napi") {
                    checked += 1;
                    assert!(
                        attrs.contains("catch_unwind"),
                        "{name}:{}: `{line}` is a #[napi] fn without catch_unwind",
                        i + 1
                    );
                }
            }
        }
        assert!(
            checked >= 29,
            "expected to check at least 29 #[napi]-attributed fns/methods, only checked {checked} \
             (a change to how attributes are written may have broken this scan's detection)"
        );
    }
}
