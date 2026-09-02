# Portfolio findings

`agent-loop-portfolio` aggregates deterministic repository findings without creating work items, issues, or agent runs.

The runner discovers Git repositories that are direct children of a portfolio root (or analyzes the root itself when it is a Git repository), executes `coding-tooling findings --json` in each repository, and preserves the returned finding objects, stable finding IDs, and analysis coverage metadata.

```sh
cargo run --bin agent-loop-portfolio -- \
  --root ~/moritzbrantner \
  --output repository-gaps.json \
  --markdown-output repository-gaps.md
```

For a canary rollout, bound the scan first:

```sh
cargo run --bin agent-loop-portfolio -- \
  --root ~/moritzbrantner \
  --limit 10 \
  --output repository-gaps.json \
  --markdown-output repository-gaps.md
```

The JSON output contains repository-level status, exit code, counts, coverage, findings, diagnostics, and portfolio summary counts grouped by repository status, finding severity, finding state, expectation, detected technology, and unsupported analysis technology. The full finding objects and coverage evidence are retained.

Coverage is essential when interpreting zero findings. The portfolio summary distinguishes:

- `coveredCleanRepositories`: repositories with zero findings, a passing findings run, no diagnostics, coverage metadata, and no known coverage gap;
- `unsupportedZeroFindingRepositories`: repositories with zero findings whose discovered technology or detector coverage remains unsupported/unavailable;
- `repositoriesWithoutCoverageMetadata`: repositories using an older findings contract or where the tool could not produce coverage metadata.

A zero-finding repository is therefore not counted as covered-clean merely because no detector emitted a finding.

The Markdown output is a reading surface rather than a second data source. It shows portfolio counts, coverage gaps, and prioritizes error findings before warnings and informational findings inside each repository. Repositories with no findings still appear when coverage is missing or unsupported. By default it shows at most 20 findings per repository; use `--max-findings-per-repository` to change that bound. Omitted findings remain present in JSON.

Use `--new-only` when the operational question is "what appeared since each repository's accepted baseline?". The runner then delegates to `coding-tooling findings --new --json`, so baseline semantics continue to belong to `coding-tooling` rather than being reimplemented by the orchestrator.

A repository that cannot execute `coding-tooling` is recorded as unavailable rather than aborting the entire scan. A repository with blocking findings retains the `failed` status and its findings; exit code 1 is therefore evidence, not a portfolio-runner failure.

`CODING_TOOLING_BIN` can point to an explicit `coding-tooling` executable. Otherwise the runner first uses an installed `coding-tooling` from `PATH`, then falls back to the registered `coding-tooling` source checkout and invokes its `src/entry.ts` through Bun. This is important because repository findings are entrypoint-level commands rather than legacy `src/cli.ts` commands.

This command is intentionally read-only. Turning selected findings into work items, GitHub issues, or probabilistic agent analysis is a separate orchestration step.
