# Vendored TOON spec encode fixtures

The JSON files under `encode/` are copied unmodified from the official TOON specification repository, for `tests/toon_spec_fixtures.rs`.

- **Source:** <https://github.com/toon-format/spec/tree/d09c3d33e37389345a74bd96981eb96dd9be6c5e/tests/fixtures/encode>
- **Commit:** `d09c3d33e37389345a74bd96981eb96dd9be6c5e` (2026-10-08), matching SPEC.md Version 4.4 (Working Draft, 2026-10-08)
- **Retrieved:** 2026-10-21
- **License:** MIT, reproduced below

michi is not a general JSON-to-TOON encoder, so the runner maps each fixture whose input falls inside michi's subset (flat tabular lists, empty lists, lists of empty objects, string arrays, primitives, flat objects of primitives) onto the michi API that produces that shape, and records a skip reason for every other case. To update, copy the files from a newer commit, update the commit above, and adjust the runner's executed-case floor.

## License

```text
MIT License

Copyright (c) 2025-PRESENT Johann Schopplich

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
