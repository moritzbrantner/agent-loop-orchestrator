# React conventions

## REACT-001 — Colocate components and directly related artifacts

- Keep a component and its focused tests, styles, hooks, and types in their smallest shared directory.

## REACT-002 — Keep React state local by default

- Own state in the smallest subtree that needs it; widen only for real shared ownership.

## REACT-003 — Put important navigational state in URL query parameters

- Put durable, shareable view state in query parameters; keep ephemeral and sensitive state out of URLs.

## REACT-004 — Use effects for external synchronization

- Use effects for systems outside React, not derived values or ordinary control flow.

## REACT-005 — Prefer composition over highly configurable mega-components

- Give a reusable component one coherent responsibility and compose larger experiences from smaller parts.
- Prefer children, slots, focused subcomponents, and small semantic wrappers over unrelated boolean flags, mode switches, and render callbacks that turn one component into several products at once.
- When variants share behavior or state but differ in presentation, keep the shared behavior in the narrowest hook/controller/headless layer and compose the visual pieces separately.

## REACT-007 — Reuse shared UI before creating local primitives

- Inspect and reuse the established UI package before creating local primitives.
- Keep workflows and stateful page composition in the app; keep reusable state-light primitives and patterns shared.

## REACT-008 — Separate update domains by frequency

- Keep fast-changing state in the smallest component, subscription, or renderer boundary that actually consumes it; do not make broad parents or providers rerender at animation-frame, pointer-hover, streaming, or simulation-tick cadence without a concrete need.
- In render loops such as React Three Fiber, prefer frame callbacks, refs, or an imperative renderer adapter for per-frame transforms. Use React state when the changed value should affect React-rendered UI.
- Split contexts or external-store subscriptions when consumers need different update frequencies; a convenient global provider is not sufficient reason to couple their rerenders.

## REACT-009 — Avoid redundant state and synchronization

- Derive values from current props, state, URL state, or authoritative external data when practical instead of copying them into additional React state.
- Do not use effects to mirror one React value into another. Effects synchronize React with systems outside React and must clean up subscriptions, timers, requests, and other external work.
- For mutable external sources, expose focused subscriptions such as `useSyncExternalStore` or selector-based stores rather than polling or forcing unrelated component trees to rerender.

## REACT-010 — Optimize boundaries before memoization

- Reduce work first through ownership boundaries, focused subscriptions, virtualization, and data shaping. Do not blanket a component tree with `memo`, `useMemo`, or `useCallback`.
- Add memoization when profiling or a clear identity/compute cost shows that it prevents meaningful work, and keep dependency semantics correct.
- Use stable semantic keys for mutable collections and virtualize large repeated views instead of rendering every row or item merely because React can express it.
- When making a material React performance optimization, add a representative deterministic benchmark or render-count regression check when practical; keep correctness tests separate from performance evidence.

## REACT-011 — Build component systems in layers

- Prefer a component vocabulary that composes upward: low-level primitives and behavior, reusable domain building blocks, feature-level compositions, then pages or screens.
- Keep dependencies flowing upward. A primitive or reusable building block must not import a page, feature workflow, or application-specific orchestration layer.
- Let pages and screens assemble building blocks and own workflow-specific wiring; do not make them the only place where reusable interaction or presentation logic exists.
- Keep domain semantics visible in component names and APIs. Reuse should not erase meaningful concepts merely to produce generic `Box`, `Item`, or `Thing` abstractions.

## REACT-012 — Expose explicit composition seams

- Make optional regions explicit composition points when consumers reasonably need to replace, omit, or reorder them. Prefer named subcomponents, slots, children, or small compound-component APIs over a growing collection of `showX`, `hideY`, `variant`, and `mode` props.
- Use compound components or a focused context when several child building blocks genuinely share one local state machine; do not introduce context merely to avoid passing a few stable props.
- Support controlled and uncontrolled ownership only when both are real consumer needs. Keep one state model and one transition path rather than separate controlled and internal implementations.
- Preserve accessibility semantics across composition seams: ownership of labels, focus, keyboard behavior, and ARIA relationships must remain clear when pieces are rearranged.

## REACT-013 — Extract stable reuse, not speculative universality

- Keep a domain-specific component local while its semantics are still changing. Do not generalize a one-off component only because it is large.
- Extract a shared building block when multiple consumers share the same semantic behavior or when a stable boundary is already clear; extract the smallest coherent unit that removes duplication without coupling unrelated workflows.
- Prefer thin domain wrappers around shared primitives or headless behavior over a universal component with many product-specific switches.
- When a reusable component accumulates independent feature flags or multiple unrelated state machines, split it before adding another mode.

## Child scopes

- [`nextjs/`](nextjs/)
- [`moritzbrantner-ui/`](moritzbrantner-ui/)
- [`tanstack-query/`](tanstack-query/)
- [`react-hook-form/`](react-hook-form/)
- [`zustand/`](zustand/)
- [`testing-library/`](testing-library/)

These are sibling specializations of React and may be composed together when a project uses several of them.
