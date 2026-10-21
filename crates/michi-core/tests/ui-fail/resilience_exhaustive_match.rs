fn main() {
    let done = michi_core::resilience::already_done(None);
    match done {
        michi_core::resilience::AlreadyDone::Yes { result } => {
            let _ = result;
        }
        michi_core::resilience::AlreadyDone::No => {}
    }
}
