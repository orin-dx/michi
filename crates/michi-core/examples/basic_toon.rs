//! Basic TOON list rendering example.
//! Run with: `cargo run -p michi-core --example basic_toon`

// Examples exist to print their output — the disallowed_macros ban on
// println! is scoped to library code, not demonstration binaries.
#![allow(clippy::disallowed_macros)]

use michi_core::toon::Value;
use michi_core::{AgentResponse, OutputFormat, Section};

fn main() {
    let issues = Section::toon(
        "issues",
        vec!["number".to_string(), "title".to_string(), "state".to_string()],
        vec![
            vec![Value::Int(51815), Value::from("[Bug]: Telegram plugin"), Value::from("open")],
            vec![Value::Int(51812), Value::from("dark mode request"), Value::from("open")],
            vec![Value::Int(51800), Value::from("update dependencies, bump major"), Value::from("closed")],
        ],
    )
    .total_count(Some(8771));
    let response = AgentResponse::new("issues")
        .section(issues)
        .hint("Run `gh-axi issue view <number>` to view an issue")
        .hint("Run `gh-axi issue list --state=open` to filter");

    println!("{}", response.render(OutputFormat::Text));
}
