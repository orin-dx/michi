fn main() {
    let t = michi_core::truncate::truncate("hello", 100, "x");
    match t {
        michi_core::truncate::Truncated { content, original_len, was_truncated, signal } => {
            let _ = (content, original_len, was_truncated, signal);
        }
    }
}
