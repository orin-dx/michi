---
michi-pipeline: minor
---

michi-pipeline: per-key concurrency limits. `ExecStep::concurrency_key(key)` puts a step in a key (one per step), and `ExecOptions::concurrency_limit(key, NonZeroUsize)` caps how many steps of that key run at once, within the mode's overall cap. A step whose key is full waits without holding back other steps, and freed slots go to the earliest-declared step that can run, so dispatch stays deterministic. Keys without a limit are unlimited.
