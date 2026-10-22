//! Regressions for defects found by a repo-wide sweep for the review's patterns: clone-shared declarations and
//! violation order that depended on declaration order under `Order::Key`.

use michi_graph::{GraphBuilder, Order, UnknownTargets, Violation};

#[test]
fn a_declaration_made_after_cloning_works_only_on_its_own_builder() {
    let mut original = GraphBuilder::<&str>::new();
    let shared = original.node("shared");
    let mut copy = original.clone();
    let x = original.node("x");
    let y = copy.node("y");
    // `x` is the original's second declaration; on the copy, position 1 is `y`. It must not land there.
    copy.depends_on(x, "shared", ());
    // Declarations made before the clone name the same node in both.
    copy.depends_on(shared, "y", ());
    original.depends_on(y, "shared", ());
    let copy = copy.build(UnknownTargets::Error).validate(Order::Key).expect("valid");
    let y_ix = copy.index_of("y").expect("y");
    let shared_ix = copy.index_of("shared").expect("shared");
    assert!(copy.prerequisites(y_ix).is_empty(), "the original's later token must not name the copy's y");
    assert_eq!(copy.prerequisites(shared_ix), [y_ix]);
    let original = original.build(UnknownTargets::Error).validate(Order::Key).expect("valid");
    let x_ix = original.index_of("x").expect("x");
    assert!(original.prerequisites(x_ix).is_empty(), "the copy's later token must not name the original's x");
}

fn violations_for(declarations: &[(&'static str, &[&'static str])], order: Order) -> Vec<Violation<&'static str>> {
    let mut b = GraphBuilder::<&str>::new();
    for &(key, deps) in declarations {
        let node = b.node(key);
        for &dep in deps {
            b.depends_on(node, dep, ());
        }
    }
    b.build(UnknownTargets::Error).validate(order).expect_err("invalid").into_vec()
}

#[test]
fn under_key_order_shuffling_declarations_does_not_reorder_violations() {
    let one: &[(&str, &[&str])] = &[("b", &["zz", "b", "aa"]), ("a", &["q"]), ("b", &[]), ("a", &[])];
    let two: &[(&str, &[&str])] = &[("a", &["q"]), ("b", &["aa", "b", "zz"]), ("a", &[]), ("b", &[])];
    let first = violations_for(one, Order::Key);
    assert_eq!(first, violations_for(two, Order::Key));
    assert_eq!(
        first,
        [
            Violation::DuplicateNode { key: "a" },
            Violation::DuplicateNode { key: "b" },
            Violation::UnknownDependency { dependent: "a", prerequisite: "q" },
            Violation::UnknownDependency { dependent: "b", prerequisite: "aa" },
            Violation::SelfDependency { key: "b" },
            Violation::UnknownDependency { dependent: "b", prerequisite: "zz" },
        ]
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Runtime,
    Dev,
}

#[test]
fn one_missing_key_under_several_kinds_is_reported_once() {
    let mut b = GraphBuilder::<&str, Kind>::new();
    let app = b.node("app");
    b.depends_on(app, "gone", Kind::Runtime).depends_on(app, "other", Kind::Runtime).depends_on(app, "gone", Kind::Dev);
    let found = b.build(UnknownTargets::Error).validate(Order::Declaration).expect_err("unknown").into_vec();
    assert_eq!(
        found,
        [
            Violation::UnknownDependency { dependent: "app", prerequisite: "gone" },
            Violation::UnknownDependency { dependent: "app", prerequisite: "other" },
        ]
    );
}
