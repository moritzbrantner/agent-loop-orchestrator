# Tooling conventions

## BUN-001 — Use Bun as the default JavaScript toolchain

- Use Bun for packages, scripts, and JavaScript/TypeScript where required tooling supports it.
- A JavaScript/TypeScript repository uses exactly one package manager and one corresponding lockfile family; do not keep npm, pnpm, Yarn, and Bun lockfiles side by side.
- Declare the package manager with an exact `bun@x.y.z` version rather than a floating range.
- A repository may deliberately choose another package manager for a concrete compatibility/tooling reason, but that is a repository-level exception rather than a second package-manager path.

## TAILWIND-001 — Prefer Tailwind CSS when practical

- Prefer Tailwind for application styling when utility classes preserve ownership near the markup.

## TAILWIND-002 — Use semantic tokens and named variants

- Use semantic tokens and named variants for visual decisions.
- Do not default to arbitrary radii, shadows, gradients, blur, or raw palette colors.

## Child scopes

- [`vite/`](vite/)
- [`storybook/`](storybook/)
- [`playwright/`](playwright/)
- [`lighthouse/`](lighthouse/)
- [`vitest/`](vitest/)
