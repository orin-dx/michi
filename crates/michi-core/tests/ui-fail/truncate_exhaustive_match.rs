fn main() {
    let t = michi_core::truncate::truncate("hello", 100);
    match t {
        michi_core::truncate::Truncated { content, original_chars, removed } => {
            let _ = (content, original_chars, removed);
        }
    }
}
