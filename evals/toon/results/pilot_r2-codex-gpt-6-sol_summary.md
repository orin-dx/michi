# Retrieval pilot: pilot_r2-codex-gpt-6-sol.jsonl

1410 records, 0 command errors/timeouts (scored as wrong)

### Accuracy by format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| json_min | 273 | 282 | 96.8% |
| json_pretty | 274 | 282 | 97.2% |
| md_kv | 271 | 282 | 96.1% |
| md_table | 272 | 282 | 96.5% |
| toon | 271 | 282 | 96.1% |

### Accuracy by dataset x format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| code_search / json_min | 30 | 30 | 100.0% |
| code_search / json_pretty | 29 | 30 | 96.7% |
| code_search / md_kv | 30 | 30 | 100.0% |
| code_search / md_table | 30 | 30 | 100.0% |
| code_search / toon | 28 | 30 | 93.3% |
| dependency_audit / json_min | 31 | 33 | 93.9% |
| dependency_audit / json_pretty | 32 | 33 | 97.0% |
| dependency_audit / md_kv | 31 | 33 | 93.9% |
| dependency_audit / md_table | 29 | 33 | 87.9% |
| dependency_audit / toon | 31 | 33 | 93.9% |
| file_listing / json_min | 25 | 30 | 83.3% |
| file_listing / json_pretty | 26 | 30 | 86.7% |
| file_listing / md_kv | 24 | 30 | 80.0% |
| file_listing / md_table | 25 | 30 | 83.3% |
| file_listing / toon | 24 | 30 | 80.0% |
| github_issues / json_min | 33 | 33 | 100.0% |
| github_issues / json_pretty | 33 | 33 | 100.0% |
| github_issues / md_kv | 32 | 33 | 97.0% |
| github_issues / md_table | 33 | 33 | 100.0% |
| github_issues / toon | 33 | 33 | 100.0% |
| ledger_bigids / json_min | 28 | 30 | 93.3% |
| ledger_bigids / json_pretty | 30 | 30 | 100.0% |
| ledger_bigids / md_kv | 29 | 30 | 96.7% |
| ledger_bigids / md_table | 30 | 30 | 100.0% |
| ledger_bigids / toon | 29 | 30 | 96.7% |
| package_search / json_min | 30 | 30 | 100.0% |
| package_search / json_pretty | 28 | 30 | 93.3% |
| package_search / md_kv | 29 | 30 | 96.7% |
| package_search / md_table | 29 | 30 | 96.7% |
| package_search / toon | 30 | 30 | 100.0% |
| service_status / json_min | 30 | 30 | 100.0% |
| service_status / json_pretty | 30 | 30 | 100.0% |
| service_status / md_kv | 30 | 30 | 100.0% |
| service_status / md_table | 30 | 30 | 100.0% |
| service_status / toon | 30 | 30 | 100.0% |
| sparse_contacts / json_min | 36 | 36 | 100.0% |
| sparse_contacts / json_pretty | 36 | 36 | 100.0% |
| sparse_contacts / md_kv | 36 | 36 | 100.0% |
| sparse_contacts / md_table | 36 | 36 | 100.0% |
| sparse_contacts / toon | 36 | 36 | 100.0% |
| test_failures / json_min | 30 | 30 | 100.0% |
| test_failures / json_pretty | 30 | 30 | 100.0% |
| test_failures / md_kv | 30 | 30 | 100.0% |
| test_failures / md_table | 30 | 30 | 100.0% |
| test_failures / toon | 30 | 30 | 100.0% |

### Accuracy by question kind x format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| argmax / json_min | 21 | 21 | 100.0% |
| argmax / json_pretty | 21 | 21 | 100.0% |
| argmax / md_kv | 21 | 21 | 100.0% |
| argmax / md_table | 21 | 21 | 100.0% |
| argmax / toon | 21 | 21 | 100.0% |
| count / json_min | 29 | 30 | 96.7% |
| count / json_pretty | 28 | 30 | 93.3% |
| count / md_kv | 29 | 30 | 96.7% |
| count / md_table | 29 | 30 | 96.7% |
| count / toon | 28 | 30 | 93.3% |
| count_filter / json_min | 21 | 24 | 87.5% |
| count_filter / json_pretty | 22 | 24 | 91.7% |
| count_filter / md_kv | 19 | 24 | 79.2% |
| count_filter / md_table | 22 | 24 | 91.7% |
| count_filter / toon | 24 | 24 | 100.0% |
| filter_list / json_min | 27 | 27 | 100.0% |
| filter_list / json_pretty | 27 | 27 | 100.0% |
| filter_list / md_kv | 27 | 27 | 100.0% |
| filter_list / md_table | 27 | 27 | 100.0% |
| filter_list / toon | 26 | 27 | 96.3% |
| lookup / json_min | 85 | 87 | 97.7% |
| lookup / json_pretty | 87 | 87 | 100.0% |
| lookup / md_kv | 85 | 87 | 97.7% |
| lookup / md_table | 86 | 87 | 98.9% |
| lookup / toon | 84 | 87 | 96.6% |
| max / json_min | 6 | 6 | 100.0% |
| max / json_pretty | 6 | 6 | 100.0% |
| max / md_kv | 6 | 6 | 100.0% |
| max / md_table | 6 | 6 | 100.0% |
| max / toon | 6 | 6 | 100.0% |
| min / json_min | 21 | 24 | 87.5% |
| min / json_pretty | 21 | 24 | 87.5% |
| min / md_kv | 21 | 24 | 87.5% |
| min / md_table | 21 | 24 | 87.5% |
| min / toon | 21 | 24 | 87.5% |
| nested / json_min | 3 | 3 | 100.0% |
| nested / json_pretty | 3 | 3 | 100.0% |
| nested / md_kv | 3 | 3 | 100.0% |
| nested / md_table | 3 | 3 | 100.0% |
| nested / toon | 3 | 3 | 100.0% |
| null_count / json_min | 6 | 6 | 100.0% |
| null_count / json_pretty | 5 | 6 | 83.3% |
| null_count / md_kv | 6 | 6 | 100.0% |
| null_count / md_table | 3 | 6 | 50.0% |
| null_count / toon | 4 | 6 | 66.7% |
| null_lookup / json_min | 3 | 3 | 100.0% |
| null_lookup / json_pretty | 3 | 3 | 100.0% |
| null_lookup / md_kv | 3 | 3 | 100.0% |
| null_lookup / md_table | 3 | 3 | 100.0% |
| null_lookup / toon | 3 | 3 | 100.0% |
| positional / json_min | 27 | 27 | 100.0% |
| positional / json_pretty | 27 | 27 | 100.0% |
| positional / md_kv | 27 | 27 | 100.0% |
| positional / md_table | 27 | 27 | 100.0% |
| positional / toon | 27 | 27 | 100.0% |
| reverse / json_min | 24 | 24 | 100.0% |
| reverse / json_pretty | 24 | 24 | 100.0% |
| reverse / md_kv | 24 | 24 | 100.0% |
| reverse / md_table | 24 | 24 | 100.0% |
| reverse / toon | 24 | 24 | 100.0% |
