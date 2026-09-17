# Performance Evidence integration

`agent-loop-orchestrator` owns durable task/run state and the rich execution ledger. It does not own the portable computational-cost vocabulary.

The canonical contract and `agent-run/v1` measurement profile are owned by [`moritzbrantner/performance-evidence`](https://github.com/moritzbrantner/performance-evidence). The local `agent-loop-performance-evidence` adapter implements that public profile over the existing `agent-loop-efficiency` report; it does not introduce another task database or another performance schema.

## Export flow

First produce the richer ledger report for the desired window:

```sh
agent-loop-efficiency --since 2026-09-10T00:00:00Z --until 2026-09-17T00:00:00Z --output .artifacts/agent-efficiency.json >/dev/null
```

Then derive canonical per-attempt artifacts:

```sh
agent-loop-performance-evidence \
  --report .artifacts/agent-efficiency.json \
  --output-dir .artifacts/performance-evidence/attempts \
  --source-dirty false
```

`--source-dirty` is intentionally explicit. Pass `false` only when the execution layer has established that the source baseline for the exported attempts was clean; the adapter will not infer cleanliness from unrelated current checkout state.

The rich efficiency report should normally be retained alongside the canonical artifacts. The former is better for orchestrator-specific investigation; the latter is the stable input for cross-provider/model comparison, Performance Evidence rollups, runtime-profiler views, and architecture-review skills.

## Contract parity

The adapter fixture is copied from the authoritative `performance-evidence` agent-loop adapter fixture and asserts byte-semantic JSON parity, including workload/environment fingerprints. Provider session IDs are deliberately excluded from portable evidence, and absent token/cost/time telemetry remains absent.

The adapter converts the already-exported report in one pass. It does not reload the execution ledger or provider event streams; a 2,000-attempt deterministic smoke test exercises that bounded conversion path without introducing a brittle wall-clock CI threshold.
