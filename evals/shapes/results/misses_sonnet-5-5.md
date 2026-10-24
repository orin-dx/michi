# Wrong answers: sonnet-5-5

| id | format | kind | expected | got |
|---|---|---|---|---|
| sparse-06 | json_pretty | filter_count | `3` | `Enterprise rows: Sven Hale (38), Quin Ito (null), Cleo Bauer (44), Kira Bauer ...` |
| sparse-10 | json_pretty | multihop | `"Pune"` | `Highest mrr: 998.92 (Gus Frost) vs 998.25 (Quin Ito). Gus Frost's city is Pune. ...` |
| sparse-11 | json_min | multihop | `2` | `Plan pro rows: Sven Egan (Lyon) yes; Faye Jain (city null) no; Mina Ito (Quito) ...` |
| tree-04 | toon_ref | filter_count | `6` | `9` |
| tree-05 | md | filter_count | `3` | `Deepest level: repo/src/render/theme/ (depth 4 directories: repo, src, render, ...` |
| tree-05 | xml | filter_count | `3` | `Deepest level: repo/src/render/theme (depth 3 directories below repo) contains ...` |
| tree-05 | yaml | filter_count | `3` | `Deepest level: src/render/theme (depth 3 directories: src, render, theme), ...` |
| tree-06 | json_pretty | aggregate | `520293` | `Sum: mod.rs 4292 + table.rs 621 + cells.rs 200995 + escape.rs 5753 + theme ...` |
| tree-06 | md | aggregate | `520293` | `Files under src/render: mod.rs 4292, table.rs 621, cells.rs 200995, escape.rs ...` |
| tree-07 | json_pretty | aggregate | `"api.md"` | `main.rs` |
| tree-07 | md | aggregate | `"api.md"` | `main.rs` |
| tree-07 | toon_ref | aggregate | `"api.md"` | `main.rs` |
| tree-07 | xml | aggregate | `"api.md"` | `main.rs` |
| tree-07 | yaml | aggregate | `"api.md"` | `main.rs` |
| tree-08 | toon_ref | multihop | `"src"` | `.rs files: main.rs 332228, default.rs 303195, lexer.rs 254094, errors.rs ...` |
| tree-08 | yaml | multihop | `"src"` | `main.rs is 332228 in src. Others: default.rs 303195, lexer.rs 254094. Largest ...` |
| tree-09 | md | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme/ = 5 5` |
| tree-09 | xml | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme (dir) = 5 5` |
| tree-09 | yaml | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme → 5 **5**` |
| tree-12 | json_min | exact_copy | `"src/parse/errors.rs"` | `src/parse/lexer.rs` |
| tree-12 | toon_ref | exact_copy | `"src/parse/errors.rs"` | `src/parse/lexer.rs` |
| uniform-03 | toon | lookup | `"v2.19.2"` | `Searching auth in eu-west-1: the only auth rows are ap-south-1, eu-west-1 ...` |
| uniform-05 | json_min | filter_count | `2` | `eu-west-1 entries: auth degraded, checkout degraded, orders failed, search ...` |
| uniform-05 | json_pretty | filter_count | `2` | `eu-west-1 deployments: auth (degraded), reports (running), checkout (degraded), ...` |
| uniform-07 | json_min | aggregate | `1` | `Degraded replicas: 11, 6, 6, 11, 9, 10, 7, 9, 1, 4, 11 → lowest is 1. 1` |
| uniform-07 | json_pretty | aggregate | `1` | `Degraded replicas: 11, 6, 6, 9, 10, 7, 9, 1, 11, 4, 11. Lowest is 1. 1` |
| uniform-07 | md | aggregate | `1` | `Degraded replicas: 11, 6, 6, 11, 10, 7, 9, 1, 4, 11 → lowest is 1. 1` |
| uniform-07 | toon | aggregate | `1` | `Degraded replicas: 11, 6, 6, 11, 10, 7, 9, 1, 4, 11. Lowest is 1. 1` |
| uniform-12 | json_min | exact_copy | `"814179278214594a2f4c65be"` | `orders with cpu_pct 1: uid 814179278214594a2f4c65be (cpu 1). Check others: ...` |
