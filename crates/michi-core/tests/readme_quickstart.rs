//! The README's Rust quick start must compile as a pasted program and print exactly the README's `### Output` block.
//!
//! `examples/readme_quickstart.rs` is the snippet verbatim (checked against the README below), so `cargo build
//! --examples` proves it compiles as a standalone program. This test then runs that same source, with `print!`
//! redirected into a buffer, and compares what it printed with the README's shown output.

const README: &str = include_str!("../../../README.md");
const EXAMPLE: &str = include_str!("../examples/readme_quickstart.rs");

/// The body of the first fenced block opened by `fence` (e.g. "```rust") after the `heading` line.
fn fenced_block_after(heading: &str, fence: &str) -> &'static str {
    let start = README.find(heading).unwrap_or_else(|| panic!("README has no {heading:?} heading"));
    let rest = &README[start..];
    let open = rest.find(&format!("{fence}\n")).unwrap_or_else(|| panic!("no {fence} block after {heading:?}"));
    let body = &rest[open + fence.len() + 1..];
    let close = body.find("```").unwrap_or_else(|| panic!("unterminated {fence} block after {heading:?}"));
    &body[..close]
}

mod quickstart {
    use std::cell::RefCell;

    thread_local! {
        pub static OUT: RefCell<String> = const { RefCell::new(String::new()) };
    }

    // Shadows the prelude `print!` for the included source below, so its output can be compared.
    macro_rules! print {
        // Format outside the closure, so a `?` in the arguments still returns from the caller.
        ($($arg:tt)*) => {{
            let text = format!($($arg)*);
            OUT.with(|o| o.borrow_mut().push_str(&text));
        }};
    }

    include!("../examples/readme_quickstart.rs");

    pub fn run() -> String {
        main().expect("the README quick start returns Ok");
        OUT.with(|o| o.borrow().clone())
    }
}

#[test]
fn readme_rust_quick_start_is_the_compiled_example_verbatim() {
    let snippet = fenced_block_after("### Rust", "```rust");
    assert_eq!(snippet, EXAMPLE, "README's Rust quick start must match examples/readme_quickstart.rs exactly");
}

#[test]
fn readme_output_block_matches_what_the_quick_start_prints() {
    let shown = fenced_block_after("### Output", "```text");
    let printed = quickstart::run();
    // The fenced block ends with the newline before the closing fence; `print!` writes no trailing newline.
    assert_eq!(format!("{printed}\n"), shown);
}
