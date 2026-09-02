# Portfolio findings

`agent-loop-portfolio` aggregates deterministic repository findings without creating work items, issues, or agent runs.

The runner discovers Git repositories that are direct children of a portfolio root (or analyzes the root itself when it is a Git repository), executes `coding-tooling findings --json` in each repository, and preserves the returned finding objects and stable finding IDs.

```sh
cargo run --bin agent-loop-portfolio -- \
  --root ~/moritzbrantner \
  --output repository-gaps.json
```

For a canary rollout, bound the scan first:

```sh
cargo run --bin agent-loop-portfolio -- \
  --root ~/moritzbrantner \
  --limit 10 \
  --output repository-gaps.json
```

The output contains repository-level status, exit code, counts, findings, and diagnostics plus portfolio totals. A repository that cannot execute `coding-tooling` is recorded as unavailable rather than aborting the entire scan. A repository with blocking findings retains the `failed` status and its findings; exit code 1 is therefore evidence, not a portfolio-runner failure.

`CODING_TOOLING_BIN` can point to an explicit `coding-tooling` executable. Otherwise the runner uses the tool activated through the normal agent-loop environment registry / `PATH`.

This command is intentionally read-only. Turning selected findings into work items, GitHub issues, or probabilistic agent analysis is a separate orchestration step.
