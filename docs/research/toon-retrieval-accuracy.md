# TOON retrieval accuracy

Status: Open

## Question

For list data returned to an assistant, does TOON preserve retrieval accuracy compared with Markdown-KV, and does the result vary by model size?

## Evidence so far

Token-count savings have been measured for CLI/MCP workloads. Retrieval accuracy has not. The result could change which format Michi recommends; it does not block either renderer.

## Method

Use the same list data and retrieval tasks in both formats. Compare answers against a predefined scoring rule across several model sizes. Record model/version, prompt, data, token count, and per-task result so the comparison can be reproduced.

## Decision this blocks

Recommend a default format for assistant-facing list responses, or leave the choice to consumers if results are mixed. Do not claim that lower token count implies better retrieval.
