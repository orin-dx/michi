use michi_core::toon::{render_toon, ToonOptions, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = ToonOptions::new(
        "issues",
        vec!["number".to_string(), "title".to_string(), "state".to_string()],
        vec![
            vec![Value::Int(51815), Value::from("[Bug]: Telegram plugin"), Value::from("open")],
            vec![Value::Int(51812), Value::from("dark mode request"), Value::from("open")],
        ],
    )
    .total_count(Some(8771))
    .hints(vec!["Run `gh-axi issue view <number>` to view an issue".to_string()]);

    print!("{}", render_toon(&opts)?);
    Ok(())
}
