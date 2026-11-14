//! AC-036: `Health`, `StatusItem` and `StatusResponse` have no `Serialize` or `Deserialize` impl even with the
//! `serde` feature on.
//!
//! These used to be trybuild snapshots. Those pin the text rustc prints about `serde_json`'s source, so a compiler
//! release that reformats that quote broke them without any change here. This check never looks at compiler output: a
//! type that gains one of the impls makes this file stop compiling.
#![cfg(feature = "serde")]

/// Fails to compile when `$ty` implements the trait bound `$($bound)+`.
///
/// `<$ty as AmbiguousIfImpl<_>>::some_item` has one candidate impl (`A = ()`) while the bound does not hold. Once
/// it holds there are two, the inferred `A` is ambiguous, and rustc reports E0283.
macro_rules! assert_not_impl {
    ($ty:ty: $($bound:tt)+) => {
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            impl<T: ?Sized + $($bound)+> AmbiguousIfImpl<u8> for T {}
            let _ = <$ty as AmbiguousIfImpl<_>>::some_item;
        };
    };
}

assert_not_impl!(michi_core::Health: serde::Serialize);
assert_not_impl!(michi_core::StatusItem: serde::Serialize);
assert_not_impl!(michi_core::StatusResponse: serde::Serialize);
assert_not_impl!(michi_core::Health: for<'de> serde::Deserialize<'de>);
assert_not_impl!(michi_core::StatusItem: for<'de> serde::Deserialize<'de>);
assert_not_impl!(michi_core::StatusResponse: for<'de> serde::Deserialize<'de>);

/// The assertions above run at compile time; this keeps the file visible in the test list.
#[test]
fn status_types_have_no_serde_impls() {}
