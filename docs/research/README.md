# Research notes

These notes track questions that need evidence. They do not change Michi's API or guarantee a design will be adopted.

| Note | Question |
| --- | --- |
| [TOON interoperability](toon-interoperability.md) | How does Michi's documented dialect compare with independent TOON implementations? |
| [TOON retrieval accuracy](toon-retrieval-accuracy.md) | Does TOON preserve retrieval accuracy compared with Markdown-KV? Answered for Claude and OpenAI models: yes |
| [Shape x format evaluation](shape-format-eval.md) | Which body format (TOON, JSON, YAML, Markdown, TOML, XML) suits which data shape when an agent reads it? Tokens answered: TOON wins on flat tables only; accuracy inconclusive, no format separated |
| [Callisto publishing migration](callisto-publishing.md) | What must change before adopting Callisto 0.10 for versioning and publication? Adopted; now on 0.12 |
| [Pipeline design](pipeline-design.md) | What should an embeddable, replay-safe Tokio step pipeline look like? Decided in [ADR 7](../adr/0007-pipeline-plans-records-replay.md) |

When research resolves a product or architecture decision, record that decision in the relevant spec or [ADR index](../adr/README.md).
