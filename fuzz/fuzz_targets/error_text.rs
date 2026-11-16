//! Error text from untrusted sources (a `source()` chain, a pipeline step's message, an HTTP body, a panic payload)
//! converts to a `DomainError` without a panic, keeps its `message` free of the step's text, bounds the detail, never
//! leaks a planted token, and renders to valid JSON.

#![no_main]

use std::fmt;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::{DomainError, ErrorCode, OutputFormat};
use michi_pipeline::ExecutionError;

/// One link of a `source()` chain with arbitrary `Display` text.
#[derive(Debug)]
struct Link {
    text: String,
    source: Option<Box<Link>>,
}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::error::Error for Link {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|l| l as &(dyn std::error::Error + 'static))
    }
}

#[derive(Arbitrary, Debug)]
struct Input {
    chain: Vec<String>,
    step_text: String,
    status: u16,
    which: u8,
    /// Wraps the planted token in a URL, a query or bare.
    shape: u8,
}

const PLANTED: &str = "ghp_Zq9xPlantedSecret7Zq9xPlantedSecret7";

fn planted(shape: u8) -> String {
    match shape % 3 {
        0 => format!("https://bot:{PLANTED}@host/r"),
        1 => format!("https://host/r?token={PLANTED}"),
        _ => format!("Bearer {PLANTED}"),
    }
}

fuzz_target!(|input: Input| {
    // The chain: no panic, and the walk stays bounded.
    let mut link: Option<Box<Link>> = None;
    for text in input.chain.iter().take(300).rev() {
        link = Some(Box::new(Link { text: text.clone(), source: link }));
    }
    if let Some(head) = link {
        let e = DomainError::from_error_with_causes(ErrorCode::ExternalFailure, &*head);
        assert!(e.causes().len() <= 128);
        let _ = e.render(OutputFormat::Text);
        serde_json::from_str::<serde_json::Value>(&e.render(OutputFormat::Json)).expect("render_json is valid JSON");
    }

    // The pipeline conversion, with a planted token somewhere in the step text.
    let text = format!("{} {} {}", input.step_text, planted(input.shape), input.step_text);
    let err = match input.which % 6 {
        0 => ExecutionError::Http { status: input.status, message: text, retry_after: None },
        1 => ExecutionError::failed(text),
        2 => ExecutionError::Panicked { message: text },
        3 => ExecutionError::DeadlinePassed { message: text },
        4 => ExecutionError::StepFailed {
            step_id: "s".into(),
            step_name: "S".into(),
            source: Box::new(ExecutionError::failed(text)),
        },
        _ => ExecutionError::AmbiguousOutcome { source: Box::new(ExecutionError::failed(text)) },
    };
    let d: DomainError = err.into();
    assert!(d.message().len() < 200, "the trusted message stays fixed text: {}", d.message().len());
    for cause in d.causes() {
        assert!(cause.len() <= 512 + '…'.len_utf8(), "cause of {} bytes", cause.len());
    }
    assert!(!d.render(OutputFormat::Text).contains(PLANTED), "planted token survived: {}", d.render(OutputFormat::Text));
    assert!(!d.render(OutputFormat::Json).contains(PLANTED), "planted token survived in JSON");
    serde_json::from_str::<serde_json::Value>(&d.render(OutputFormat::Json)).expect("render_json is valid JSON");
});
