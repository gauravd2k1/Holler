# ADR-010: Flutter for the Waiter App

## Context
The waiter/captain app runs primarily on affordable Android phones and tablets, which dominate restaurant staff device deployments in India. It needs fast development velocity, strong Android performance, and low ongoing maintenance overhead for a small team already spanning Rust, Go, and TypeScript.

## Decision
**Flutter**, Android-first. This decision is final — no further React Native vs. Flutter evaluation is to be produced; proceed with implementation.

## Alternatives
- **React Native**: considered but not selected — Flutter's more predictable performance characteristics and single-codebase rendering model were judged a better fit for a staff-facing, performance-sensitive Android-first app. (Per the master prompt, this decision is not to be re-litigated or re-evaluated by builder agents.)
- **Native Android (Kotlin)**: rejected — no cross-platform path if iOS support is ever needed, and adds a fourth language to the stack without a clear necessity at this stage.

## Consequences
- Waiter app code lives in `apps/waiter/` as a Flutter project, consuming the same `packages/contracts/`-defined API/event shapes as every other client.
- The team now maintains Dart/Flutter alongside Rust, Go, and TypeScript — an accepted cost for staff-app velocity and Android performance.
- Any future iOS waiter app need reuses this Flutter codebase rather than a rewrite.
