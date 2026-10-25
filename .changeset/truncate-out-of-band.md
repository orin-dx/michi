---
michi-core: minor
michi-node: minor
---

`truncate()` reports the cut outside the text. The old form appended `" (N more chars — use hint)"` inside the returned string, which data could imitate, the same flaw cut TOON cells had. Now `truncate(content, max_chars)` returns `Truncated { content, original_chars, removed }`: text that fits comes back unchanged, and a longer text keeps its first `max_chars - 1` chars and ends in a plain `…`. `Truncated::hint(guidance)` gives the `cut N of M chars; …` line for the response's `help[]`.

Migrate: drop the `hint` argument; read `removed` (or call `hint(guidance)`) instead of parsing the suffix; replace `truncate_inline(c, n, h)` with `truncate(c, n).content` and report the cut yourself. In Node, `truncate(content, maxChars)` now returns `{ content, originalChars, removed, wasTruncated }` instead of a string.
