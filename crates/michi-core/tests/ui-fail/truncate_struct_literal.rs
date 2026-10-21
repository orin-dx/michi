fn main() {
    let _ = michi_core::truncate::Truncated {
        content: String::new(),
        original_len: 0,
        was_truncated: false,
        signal: None,
    };
}
