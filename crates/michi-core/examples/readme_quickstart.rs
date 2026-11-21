use michi_core::toon::Value;
use michi_core::{AgentResponse, OutputFormat, Section};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let issues = Section::toon(
        "issues",
        vec!["number".to_string(), "title".to_string(), "state".to_string()],
        vec![
            vec![Value::Int(51815), Value::from("[Bug]: Telegram plugin"), Value::from("open")],
            vec![Value::Int(51812), Value::from("dark mode request"), Value::from("open")],
        ],
    )
    .total_count(Some(8771));
    let response =
        AgentResponse::new("issues").section(issues).hint("Run `gh-axi issue view <number>` to view an issue");

    print!("{}", response.render(OutputFormat::Text));
    Ok(())
}
