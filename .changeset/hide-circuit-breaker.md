---
michi-pipeline: minor
---

`CircuitBreaker` and `BreakerRegistry::breaker()` are no longer public. A standalone breaker could be created and its phase read, but nothing could drive it; breakers are used through `ExecOptions::breakers` and a `BreakerRegistry`. Read a breaker's phase with `BreakerRegistry::phase(key)`, which returns `None` until a step has used that key. `BreakerConfig`, `BreakerPhase` and `BreakerRegistry` are unchanged.
