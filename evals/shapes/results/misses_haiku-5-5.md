# Wrong answers: haiku-5-5

| id | format | kind | expected | got |
|---|---|---|---|---|
| events-02 | json_pretty | filter_count | `7` | `Login events with success false: lena (06:25), kofi, lena (10:23), tomas, lena ...` |
| events-03 | json_pretty | filter_count | `4` | `Critical alerts: queue_depth (12:47), disk_full (20:30), latency_p99 (22:24), ...` |
| events-03 | md | filter_count | `4` | `Critical alerts: 12:47:50 queue_depth, 20:30:49 disk_full, 22:24:01 ...` |
| events-03 | toon | filter_count | `4` | `Critical alerts: queue_depth (12:47), disk_full (20:30), latency_p99 (22:24), ...` |
| events-03 | toon_ref | filter_count | `4` | `Critical alerts: queue_depth (12:47), disk_full (20:30), latency_p99 (22:24), ...` |
| events-03 | yaml | filter_count | `4` | `Critical alerts: queue_depth (12:47), disk_full (20:30), latency_p99 (22:24), ...` |
| events-04 | json_min | aggregate | `3877` | `Deploy durations: 891, 588, 390, 213, 776, 38, 430, 551. 891+588=1479; ...` |
| events-04 | json_pretty | aggregate | `3877` | `Deploy durations: 891, 588, 390, 213, 776, 38, 430, 551. 891+588=1479; ...` |
| events-04 | md | aggregate | `3877` | `Deploy durations: 891, 588, 390, 213, 776, 38, 430, 551. 891+588=1479; ...` |
| events-04 | toon_ref | aggregate | `3877` | `Deploy durations: 891, 588, 390, 213, 776, 38, 430, 551. 891+588=1479; ...` |
| events-04 | yaml | aggregate | `3877` | `Deploy durations: 891, 588, 390, 213, 776, 38, 430, 551. 891+588=1479; ...` |
| events-06 | json_min | aggregate | `"ip-10-1-15-9"` | `ip-10-1-40-40` |
| events-06 | yaml | aggregate | `"ip-10-1-15-9"` | `ip-15-9 check: alert values: 65.6, 89.1, 72.2, 86.6, 93.7, 92.2, 54.6, 82.0, ...` |
| events-07 | toon | multihop | `"lena"` | `Failed logins: lena (06:18... actually 06:25, 10:23, 23:49) = 3; kofi 1; tomas ...` |
| events-07 | yaml | multihop | `"lena"` | `Failed logins: lena 3 (06:25, 10:23, 23:49), kofi 1, tomas 1, mira 2. lena` |
| events-09 | json_min | null_count | `3` | `2` |
| events-09 | md | null_count | `3` | `Error events with empty trace_id: 03:12:23 gateway, 03:18:13 checkout, and ...` |
| events-09 | toon | null_count | `3` | `2` |
| longtext-06 | json_min | aggregate | `8` | `Shipping tickets: #103 (3), #107 (5). Sum = 8. 8` |
| longtext-06 | json_pretty | aggregate | `8` | `Shipping tickets: #103 (rating 3) and #107 (rating 5). 8` |
| longtext-06 | md | aggregate | `8` | `Shipping tickets: #103 (3), #107 (5). Sum = 8. 8` |
| longtext-06 | toon | aggregate | `8` | `Shipping tickets: #103 (3), #107 (5). Sum = 8. 8` |
| longtext-06 | yaml | aggregate | `8` | `Shipping tickets: #103 (rating 3) and #107 (rating 5). 8` |
| longtext-07 | json_min | aggregate | `1` | `Account tickets ratings: 5, 2, 5, 5, 1. Lowest is 1. 1` |
| longtext-07 | json_pretty | aggregate | `1` | `Account tickets: ratings 5, 2, 5, 5, 1. Lowest is 1. 1` |
| longtext-07 | md | aggregate | `1` | `Account tickets: #100 (5), #101 (2), #102 (5), #106 (5), #118 (1). 1` |
| longtext-07 | toon | aggregate | `1` | `Account tickets: ratings 5, 2, 5, 5, 1. Lowest is 1. 1` |
| longtext-07 | yaml | aggregate | `1` | `Account tickets: ratings 5, 2, 5, 5, 1. Lowest is 1. 1` |
| longtext-08 | yaml | null_count | `7` | `Counting null assignees: #104, #109, #110, #117, #118, #119, #120 = 7 7` |
| nested-03 | toml | filter_count | `3` | `4` |
| nested-07 | json_min | multihop | `3` | `Assignees: mira.k, tomas.v. Comments by others: dev.s, lena.r, ana.p = 3. 3` |
| nested-07 | json_pretty | multihop | `3` | `Assignees: mira.k, tomas.v. Comments by others: dev.s, lena.r, ana.p = 3. 3` |
| nested-07 | md | multihop | `3` | `Assignees: mira.k, tomas.v. Comments by others: 3 (dev.s), 5 (lena.r), 8 ...` |
| nested-07 | toml | multihop | `3` | `Assignees: mira.k, tomas.v. Non-assignee comments: dev.s, lena.r, ana.p. 3` |
| nested-07 | toon_ref | multihop | `3` | `Assignees: mira.k, tomas.v. Comments by others: dev.s, lena.r, ana.p = 3. 3` |
| nested-07 | xml | multihop | `3` | `Assignees: mira.k, tomas.v. Comments by others: dev.s, lena.r, ana.p = 3. 3` |
| nested-07 | yaml | multihop | `3` | `Assignees: mira.k, tomas.v. Non-assignee comments: dev.s, lena.r, ana.p = 3. 3` |
| nested-10 | toml | null_count | `2` | `0` |
| single-09 | toml | null_count | `2` | `0` |
| single-10 | json_min | filter_count | `5` | `Numeric fields: cpu_cores, memory_gb, disk_gb, cost_per_hour, uptime_hours. 6 ...` |
| single-10 | json_pretty | filter_count | `5` | `Numeric fields: cpu_cores, memory_gb, disk_gb, cost_per_hour, uptime_hours. 5` |
| single-10 | toml | filter_count | `5` | `Numeric fields: cpu_cores, memory_gb, disk_gb, cost_per_hour, uptime_hours. 5` |
| single-10 | toon | filter_count | `5` | `Numeric fields: cpu_cores, memory_gb, disk_gb, cost_per_hour, uptime_hours. ...` |
| sparse-06 | json_min | filter_count | `3` | `Enterprise rows: Sven Hale (38), Quin Ito (null), Cleo Bauer (44), Kira Bauer ...` |
| sparse-06 | json_pretty | filter_count | `3` | `Enterprise rows: Sven Hale (38), Quin Ito (null), Cleo Bauer (44), Kira Bauer ...` |
| sparse-10 | json_pretty | multihop | `"Pune"` | `Highest mrr is 998.92 (Gus Frost), city Pune. Pune` |
| sparse-11 | json_pretty | multihop | `2` | `Plan pro rows: Sven Egan (Lyon) yes; Faye Jain (city null) no; Mina Ito (Quito) ...` |
| tree-04 | toon_ref | filter_count | `6` | `9` |
| tree-05 | md | filter_count | `3` | `Deepest level: repo/src/render/theme/ (3 directories below repo... depth: ...` |
| tree-05 | xml | filter_count | `3` | `Deepest level: repo/src/render/theme (depth 3 below repo), containing ...` |
| tree-06 | yaml | aggregate | `520293` | `Files under src/render: mod.rs 4292, table.rs 621, cells.rs 200995, escape.rs ...` |
| tree-07 | json_min | aggregate | `"api.md"` | `Largest: docs/api.md at 367738. Others: main.rs 332228, default.rs 303195. ...` |
| tree-07 | json_pretty | aggregate | `"api.md"` | `main.rs` |
| tree-07 | md | aggregate | `"api.md"` | `main.rs` |
| tree-07 | toon_ref | aggregate | `"api.md"` | `main.rs` |
| tree-07 | xml | aggregate | `"api.md"` | `main.rs` |
| tree-07 | yaml | aggregate | `"api.md"` | `main.rs` |
| tree-09 | json_min | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme = 5. 5` |
| tree-09 | md | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme/ = 5 5` |
| tree-09 | xml | multihop | `5` | `src/render children: mod.rs, table.rs, cells.rs, escape.rs, theme (dir) = 5 5` |
| tree-12 | json_min | exact_copy | `"src/parse/errors.rs"` | `src/parse/lexer.rs is 254094; errors.rs is 269697; ast.rs 138638. Largest is ...` |
| tree-12 | toon_ref | exact_copy | `"src/parse/errors.rs"` | `src/parse/lexer.rs, 254094 vs errors.rs 269697 — errors.rs is larger. ...` |
| uniform-05 | json_min | filter_count | `2` | `eu-west-1 entries: auth degraded, checkout degraded, orders failed, reports ...` |
| uniform-05 | json_pretty | filter_count | `2` | `eu-west-1 deployments: auth (degraded), gateway (stopped), checkout (degraded), ...` |
| uniform-05 | yaml | filter_count | `2` | `eu-west-1 entries: auth (degraded), orders (failed), reports (running), ...` |
| uniform-07 | json_min | aggregate | `1` | `Degraded replicas: auth 11, orders 6, checkout 6, ingest 11, checkout 9, orders ...` |
| uniform-07 | md | aggregate | `1` | `Degraded rows: auth eu-west-1 (11), orders ap-northeast-1 (6), checkout ...` |
