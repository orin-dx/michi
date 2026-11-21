---
michi-node: minor
---

**Breaking:** the input limit per call, and per `AgentResponse`, is now 16 MiB, down from 64 MiB.

- Over the limit, a call throws `input byte total N exceeds maximum of 16777216` or `cumulative byte total N exceeds maximum of 16777216`, and the builder is unchanged. Split larger content across calls or trim it.
- Why: output can be many times larger than input (escaping, repeated names), and at 64 MiB a response could take gigabytes to render or fail only after rendering. Now an oversized response is rejected up front.
- Long cells and repeated names count toward the limit wherever the output repeats them. Typical responses (short names, cells under 200 chars) are charged about the same as before.
