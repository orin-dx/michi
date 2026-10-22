//! AgentResponse builder and MCP CallToolResult assembly example.
//! Run with: `cargo run -p michi-core --example agent_response`

// Examples exist to print their output — the disallowed_macros ban on
// println! is scoped to library code, not demonstration binaries.
#![allow(clippy::disallowed_macros)]

use michi_core::kv::{KvItem, KvValue};
use michi_core::response::{AgentResponse, OutputFormat};

fn main() {
    let response = AgentResponse::new("issue")
        .kv_items(vec![
            KvItem::new("id", KvValue::Text("issue-51815".into())),
            KvItem::new("title", KvValue::Text("[Bug]: Telegram plugin".into())),
            KvItem::new("state", KvValue::Text("open".into())),
        ])
        .hint("Run `gh-axi issue view <number>` to view an issue")
        .human_content("Issue #51815 is currently open in the Telegram plugin module.");

    println!("=== Plain Text Output ===");
    println!("{}", response.render(OutputFormat::Text));

    println!("=== MCP CallToolResult ===");
    let mcp_result = response.to_call_tool_result();
    println!("Is error: {}", mcp_result.is_error);
    println!("Content blocks count: {}", mcp_result.content.len());
    for block in &mcp_result.content {
        println!("  Block (audience: {:?}): {}", block.audience, block.text);
    }
}
