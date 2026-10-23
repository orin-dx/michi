---
michi-core: minor
---

Breaking for users of the `schemars` feature: michi-core now implements schemars 1.x's `JsonSchema` (was 0.8). Upgrade your own `schemars` dependency to 1.x; `schemars::gen::SchemaGenerator` and `schemars::schema::Schema` are now `schemars::SchemaGenerator` and `schemars::Schema`.

Generated schemas are JSON Schema 2020-12 instead of draft-07. They describe the same wire form, spelled differently: subschemas under `$defs`, `const` for unit variants, `prefixItems` for tuples, and a `$ref` beside `default` without an `allOf` wrapper.
