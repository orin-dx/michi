# Wrong answers: opus-5-5

| id | format | kind | expected | got |
|---|---|---|---|---|
| sparse-06 | md | filter_count | `3` | `Enterprise: Sven Hale 38, Quin Ito blank, Cleo Bauer 44, Kira Bauer 30. So 3. 3` |
| tree-04 | toon_ref | filter_count | `6` | `tests: render_tests.rs, parse_tests.rs, plus 4 fixtures = 6.` |
| tree-05 | json_pretty | filter_count | `3` | `Deepest: src/render/theme/ files: 3. 🤖 3` |
| tree-05 | toon_ref | filter_count | `3` | `Deepest: repo/src/render/theme/ files (3). 3` |
| tree-05 | xml | filter_count | `3` | `Deepest: repo/src/render/theme with 3 files. **3**` |
| tree-05 | yaml | filter_count | `3` | `Deepest: repo/src/render/theme/ files (3). 3` |
| tree-09 | json_min | multihop | `5` | `src/render: mod.rs, table.rs, cells.rs, escape.rs, theme = 5. 5` |
| tree-09 | json_pretty | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme = 5. **5** ...` |
| tree-09 | md | multihop | `5` | `mod.rs, table.rs, cells.rs, escape.rs, theme/ = 5 **5**` |
| tree-09 | xml | multihop | `5` | `src/render: mod.rs, table.rs, cells.rs, escape.rs, theme = 5.` |
| tree-12 | toon_ref | exact_copy | `"src/parse/errors.rs"` | `errors.rs 269697 vs lexer.rs 254094. src/parse/errors.rs` |
| uniform-07 | json_pretty | aggregate | `1` | `Degraded: auth eu-west-1 11, orders ap-ne 6, checkout eu-west-1 6, checkout ...` |
| uniform-07 | toon | aggregate | `1` | `Degraded replicas: 11,6,11,6,9,10,7,9,1,4,11 → min 1. 1` |
| uniform-08 | toon | aggregate | `"orders"` | `Max cpu 99, orders. orders` |
| uniform-12 | json_min | exact_copy | `"814179278214594a2f4c65be"` | `Lowest cpu_pct is 1: orders eu-west-1. 814179278214594a2f4c65be` |
| uniform-12 | json_pretty | exact_copy | `"814179278214594a2f4c65be"` | `Lowest cpu_pct is 1 (orders eu-west-1). 814179278214594a2f4c65be` |
| uniform-12 | md | exact_copy | `"814179278214594a2f4c65be"` | `Lowest cpu_pct is 1 (orders eu-west-1). 814179278214594a2f4c65be` |
| uniform-12 | toon | exact_copy | `"814179278214594a2f4c65be"` | `Lowest cpu_pct is 1: 814179278214594a2f4c65be. 814179278214594a2f4c65be` |
