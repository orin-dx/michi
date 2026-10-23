# Retrieval pilot: pilot_r2-opus-5-5.jsonl

1410 records, 0 command errors/timeouts (scored as wrong)

### Accuracy by format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| json_min | 276 | 282 | 97.9% |
| json_pretty | 277 | 282 | 98.2% |
| md_kv | 279 | 282 | 98.9% |
| md_table | 273 | 282 | 96.8% |
| toon | 274 | 282 | 97.2% |

### Accuracy by dataset x format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| code_search / json_min | 30 | 30 | 100.0% |
| code_search / json_pretty | 30 | 30 | 100.0% |
| code_search / md_kv | 30 | 30 | 100.0% |
| code_search / md_table | 29 | 30 | 96.7% |
| code_search / toon | 28 | 30 | 93.3% |
| dependency_audit / json_min | 33 | 33 | 100.0% |
| dependency_audit / json_pretty | 32 | 33 | 97.0% |
| dependency_audit / md_kv | 33 | 33 | 100.0% |
| dependency_audit / md_table | 33 | 33 | 100.0% |
| dependency_audit / toon | 33 | 33 | 100.0% |
| file_listing / json_min | 27 | 30 | 90.0% |
| file_listing / json_pretty | 27 | 30 | 90.0% |
| file_listing / md_kv | 28 | 30 | 93.3% |
| file_listing / md_table | 27 | 30 | 90.0% |
| file_listing / toon | 27 | 30 | 90.0% |
| github_issues / json_min | 33 | 33 | 100.0% |
| github_issues / json_pretty | 33 | 33 | 100.0% |
| github_issues / md_kv | 33 | 33 | 100.0% |
| github_issues / md_table | 33 | 33 | 100.0% |
| github_issues / toon | 33 | 33 | 100.0% |
| ledger_bigids / json_min | 30 | 30 | 100.0% |
| ledger_bigids / json_pretty | 30 | 30 | 100.0% |
| ledger_bigids / md_kv | 30 | 30 | 100.0% |
| ledger_bigids / md_table | 30 | 30 | 100.0% |
| ledger_bigids / toon | 30 | 30 | 100.0% |
| package_search / json_min | 30 | 30 | 100.0% |
| package_search / json_pretty | 30 | 30 | 100.0% |
| package_search / md_kv | 30 | 30 | 100.0% |
| package_search / md_table | 29 | 30 | 96.7% |
| package_search / toon | 27 | 30 | 90.0% |
| service_status / json_min | 30 | 30 | 100.0% |
| service_status / json_pretty | 30 | 30 | 100.0% |
| service_status / md_kv | 30 | 30 | 100.0% |
| service_status / md_table | 30 | 30 | 100.0% |
| service_status / toon | 30 | 30 | 100.0% |
| sparse_contacts / json_min | 33 | 36 | 91.7% |
| sparse_contacts / json_pretty | 35 | 36 | 97.2% |
| sparse_contacts / md_kv | 35 | 36 | 97.2% |
| sparse_contacts / md_table | 34 | 36 | 94.4% |
| sparse_contacts / toon | 36 | 36 | 100.0% |
| test_failures / json_min | 30 | 30 | 100.0% |
| test_failures / json_pretty | 30 | 30 | 100.0% |
| test_failures / md_kv | 30 | 30 | 100.0% |
| test_failures / md_table | 28 | 30 | 93.3% |
| test_failures / toon | 30 | 30 | 100.0% |

### Accuracy by question kind x format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| argmax / json_min | 21 | 21 | 100.0% |
| argmax / json_pretty | 21 | 21 | 100.0% |
| argmax / md_kv | 21 | 21 | 100.0% |
| argmax / md_table | 21 | 21 | 100.0% |
| argmax / toon | 21 | 21 | 100.0% |
| count / json_min | 30 | 30 | 100.0% |
| count / json_pretty | 30 | 30 | 100.0% |
| count / md_kv | 30 | 30 | 100.0% |
| count / md_table | 29 | 30 | 96.7% |
| count / toon | 28 | 30 | 93.3% |
| count_filter / json_min | 22 | 24 | 91.7% |
| count_filter / json_pretty | 23 | 24 | 95.8% |
| count_filter / md_kv | 23 | 24 | 95.8% |
| count_filter / md_table | 22 | 24 | 91.7% |
| count_filter / toon | 24 | 24 | 100.0% |
| filter_list / json_min | 27 | 27 | 100.0% |
| filter_list / json_pretty | 27 | 27 | 100.0% |
| filter_list / md_kv | 27 | 27 | 100.0% |
| filter_list / md_table | 27 | 27 | 100.0% |
| filter_list / toon | 27 | 27 | 100.0% |
| lookup / json_min | 87 | 87 | 100.0% |
| lookup / json_pretty | 87 | 87 | 100.0% |
| lookup / md_kv | 87 | 87 | 100.0% |
| lookup / md_table | 87 | 87 | 100.0% |
| lookup / toon | 87 | 87 | 100.0% |
| max / json_min | 6 | 6 | 100.0% |
| max / json_pretty | 6 | 6 | 100.0% |
| max / md_kv | 6 | 6 | 100.0% |
| max / md_table | 6 | 6 | 100.0% |
| max / toon | 6 | 6 | 100.0% |
| min / json_min | 21 | 24 | 87.5% |
| min / json_pretty | 21 | 24 | 87.5% |
| min / md_kv | 22 | 24 | 91.7% |
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
| null_count / md_table | 6 | 6 | 100.0% |
| null_count / toon | 6 | 6 | 100.0% |
| null_lookup / json_min | 3 | 3 | 100.0% |
| null_lookup / json_pretty | 3 | 3 | 100.0% |
| null_lookup / md_kv | 3 | 3 | 100.0% |
| null_lookup / md_table | 3 | 3 | 100.0% |
| null_lookup / toon | 3 | 3 | 100.0% |
| positional / json_min | 26 | 27 | 96.3% |
| positional / json_pretty | 27 | 27 | 100.0% |
| positional / md_kv | 27 | 27 | 100.0% |
| positional / md_table | 25 | 27 | 92.6% |
| positional / toon | 27 | 27 | 100.0% |
| reverse / json_min | 24 | 24 | 100.0% |
| reverse / json_pretty | 24 | 24 | 100.0% |
| reverse / md_kv | 24 | 24 | 100.0% |
| reverse / md_table | 23 | 24 | 95.8% |
| reverse / toon | 21 | 24 | 87.5% |
