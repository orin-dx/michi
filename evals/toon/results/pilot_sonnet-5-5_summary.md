# Retrieval pilot: pilot_sonnet-5-5.jsonl

470 records, 0 command errors/timeouts (scored as wrong)

### Accuracy by format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| json_min | 83 | 94 | 88.3% |
| json_pretty | 87 | 94 | 92.6% |
| md_kv | 85 | 94 | 90.4% |
| md_table | 86 | 94 | 91.5% |
| toon | 78 | 94 | 83.0% |

### Accuracy by dataset x format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| code_search / json_min | 7 | 10 | 70.0% |
| code_search / json_pretty | 9 | 10 | 90.0% |
| code_search / md_kv | 9 | 10 | 90.0% |
| code_search / md_table | 10 | 10 | 100.0% |
| code_search / toon | 8 | 10 | 80.0% |
| dependency_audit / json_min | 11 | 11 | 100.0% |
| dependency_audit / json_pretty | 11 | 11 | 100.0% |
| dependency_audit / md_kv | 11 | 11 | 100.0% |
| dependency_audit / md_table | 11 | 11 | 100.0% |
| dependency_audit / toon | 11 | 11 | 100.0% |
| file_listing / json_min | 9 | 10 | 90.0% |
| file_listing / json_pretty | 9 | 10 | 90.0% |
| file_listing / md_kv | 9 | 10 | 90.0% |
| file_listing / md_table | 8 | 10 | 80.0% |
| file_listing / toon | 7 | 10 | 70.0% |
| github_issues / json_min | 11 | 11 | 100.0% |
| github_issues / json_pretty | 11 | 11 | 100.0% |
| github_issues / md_kv | 10 | 11 | 90.9% |
| github_issues / md_table | 11 | 11 | 100.0% |
| github_issues / toon | 9 | 11 | 81.8% |
| ledger_bigids / json_min | 7 | 10 | 70.0% |
| ledger_bigids / json_pretty | 9 | 10 | 90.0% |
| ledger_bigids / md_kv | 7 | 10 | 70.0% |
| ledger_bigids / md_table | 8 | 10 | 80.0% |
| ledger_bigids / toon | 7 | 10 | 70.0% |
| package_search / json_min | 9 | 10 | 90.0% |
| package_search / json_pretty | 10 | 10 | 100.0% |
| package_search / md_kv | 10 | 10 | 100.0% |
| package_search / md_table | 9 | 10 | 90.0% |
| package_search / toon | 8 | 10 | 80.0% |
| service_status / json_min | 10 | 10 | 100.0% |
| service_status / json_pretty | 9 | 10 | 90.0% |
| service_status / md_kv | 10 | 10 | 100.0% |
| service_status / md_table | 9 | 10 | 90.0% |
| service_status / toon | 8 | 10 | 80.0% |
| sparse_contacts / json_min | 10 | 12 | 83.3% |
| sparse_contacts / json_pretty | 10 | 12 | 83.3% |
| sparse_contacts / md_kv | 10 | 12 | 83.3% |
| sparse_contacts / md_table | 12 | 12 | 100.0% |
| sparse_contacts / toon | 12 | 12 | 100.0% |
| test_failures / json_min | 9 | 10 | 90.0% |
| test_failures / json_pretty | 9 | 10 | 90.0% |
| test_failures / md_kv | 9 | 10 | 90.0% |
| test_failures / md_table | 8 | 10 | 80.0% |
| test_failures / toon | 8 | 10 | 80.0% |

### Accuracy by question kind x format

| group | correct | n | accuracy |
|---|---:|---:|---:|
| argmax / json_min | 4 | 7 | 57.1% |
| argmax / json_pretty | 7 | 7 | 100.0% |
| argmax / md_kv | 6 | 7 | 85.7% |
| argmax / md_table | 3 | 7 | 42.9% |
| argmax / toon | 3 | 7 | 42.9% |
| count / json_min | 10 | 10 | 100.0% |
| count / json_pretty | 10 | 10 | 100.0% |
| count / md_kv | 10 | 10 | 100.0% |
| count / md_table | 10 | 10 | 100.0% |
| count / toon | 10 | 10 | 100.0% |
| count_filter / json_min | 8 | 8 | 100.0% |
| count_filter / json_pretty | 8 | 8 | 100.0% |
| count_filter / md_kv | 8 | 8 | 100.0% |
| count_filter / md_table | 8 | 8 | 100.0% |
| count_filter / toon | 8 | 8 | 100.0% |
| filter_list / json_min | 9 | 9 | 100.0% |
| filter_list / json_pretty | 9 | 9 | 100.0% |
| filter_list / md_kv | 9 | 9 | 100.0% |
| filter_list / md_table | 9 | 9 | 100.0% |
| filter_list / toon | 9 | 9 | 100.0% |
| lookup / json_min | 24 | 29 | 82.8% |
| lookup / json_pretty | 26 | 29 | 89.7% |
| lookup / md_kv | 26 | 29 | 89.7% |
| lookup / md_table | 28 | 29 | 96.6% |
| lookup / toon | 23 | 29 | 79.3% |
| max / json_min | 2 | 2 | 100.0% |
| max / json_pretty | 2 | 2 | 100.0% |
| max / md_kv | 2 | 2 | 100.0% |
| max / md_table | 2 | 2 | 100.0% |
| max / toon | 2 | 2 | 100.0% |
| min / json_min | 8 | 8 | 100.0% |
| min / json_pretty | 7 | 8 | 87.5% |
| min / md_kv | 8 | 8 | 100.0% |
| min / md_table | 7 | 8 | 87.5% |
| min / toon | 7 | 8 | 87.5% |
| nested / json_min | 1 | 1 | 100.0% |
| nested / json_pretty | 1 | 1 | 100.0% |
| nested / md_kv | 1 | 1 | 100.0% |
| nested / md_table | 1 | 1 | 100.0% |
| nested / toon | 1 | 1 | 100.0% |
| null_count / json_min | 2 | 2 | 100.0% |
| null_count / json_pretty | 2 | 2 | 100.0% |
| null_count / md_kv | 2 | 2 | 100.0% |
| null_count / md_table | 2 | 2 | 100.0% |
| null_count / toon | 2 | 2 | 100.0% |
| null_lookup / json_min | 0 | 1 | 0.0% |
| null_lookup / json_pretty | 0 | 1 | 0.0% |
| null_lookup / md_kv | 0 | 1 | 0.0% |
| null_lookup / md_table | 1 | 1 | 100.0% |
| null_lookup / toon | 1 | 1 | 100.0% |
| positional / json_min | 8 | 9 | 88.9% |
| positional / json_pretty | 8 | 9 | 88.9% |
| positional / md_kv | 6 | 9 | 66.7% |
| positional / md_table | 8 | 9 | 88.9% |
| positional / toon | 6 | 9 | 66.7% |
| reverse / json_min | 7 | 8 | 87.5% |
| reverse / json_pretty | 7 | 8 | 87.5% |
| reverse / md_kv | 7 | 8 | 87.5% |
| reverse / md_table | 7 | 8 | 87.5% |
| reverse / toon | 6 | 8 | 75.0% |
