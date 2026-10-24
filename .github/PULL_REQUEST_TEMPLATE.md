## Type of change

- [ ] Bug fix
- [ ] New primitive or feature
- [ ] Breaking change
- [ ] Docs / chore

## Why

What need or bug this addresses — the diff already shows what changed.

## Test Plan

- [ ] `just check-all` passes (formatting, clippy, rustdoc, tests)
- [ ] New behavior has test coverage (`insta` snapshot, `proptest`, or unit test as appropriate)
- [ ] Public API changed: the spec/docs are updated in this PR (`docs/spec/`, `docs/adr/`, a README), or the body has a `No spec change needed: <reason>` line
- [ ] Tests are black-box where the behavior is visible from outside (public API, built binary, wire output)
- [ ] New feature: acceptance criteria were written before the code
- [ ] If touching `packages/michi-node` or `crates/michi-core/src/napi.rs`: Node.js NAPI tests pass too

## Related

Closes #<issue>
