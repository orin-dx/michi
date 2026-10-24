//! `RunId::unique` must not collide across hosts: containers started in the same millisecond share pid 1 and counter 0,
//! and the run id prefixes the idempotency key, so a collision would dedupe new work against old downstream. The id
//! therefore carries 64 random bits.

use std::collections::BTreeSet;

use michi_pipeline::{RunId, MAX_ID_LEN};

#[test]
fn a_unique_id_ends_in_a_random_segment_and_is_still_a_valid_run_id() {
    for _ in 0..200 {
        let id = RunId::unique("deploy").unwrap();
        let parts: Vec<&str> = id.as_str().split('-').collect();
        assert_eq!(parts.len(), 5, "prefix, millis, pid, counter, random: {id}");
        assert_eq!(parts[0], "deploy");
        for part in &parts[1..] {
            assert!(part.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()), "base 36: {id}");
        }
        // 64 random bits are about 12 to 13 base-36 digits; fewer than 8 would be a 1-in-100-million draw.
        assert!(parts[4].len() >= 8, "the last segment is random bits, not a counter: {id}");
        assert!(id.as_str().len() <= MAX_ID_LEN);
        assert_eq!(RunId::try_new(id.to_string()).unwrap(), id, "the id round-trips through the validator");
    }
}

#[test]
fn the_random_segment_differs_between_ids_made_in_a_row() {
    // In one process the counter already separates these; the random part must vary independently of it, which is what
    // separates two processes that start with the same pid, counter and millisecond.
    let randoms: BTreeSet<String> =
        (0..1000).map(|_| RunId::unique("r").unwrap().as_str().rsplit('-').next().unwrap().to_string()).collect();
    assert_eq!(randoms.len(), 1000, "no two random segments repeat");
}

#[test]
fn ids_are_unique_across_many_calls_and_threads() {
    let handles: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| (0..2000).map(|_| RunId::unique("job").unwrap().to_string()).collect::<Vec<_>>())
        })
        .collect();
    let all: Vec<String> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
    let distinct: BTreeSet<&String> = all.iter().collect();
    assert_eq!(distinct.len(), all.len());
}
