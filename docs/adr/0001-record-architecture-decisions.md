# 0001. Record architecture decisions

- **Status:** accepted
- **Date:** 2026-10-19

## Context

Kiln touches security-sensitive parts of three operating systems. Contributors need to know
why things are built the way they are, especially for choices that look like they could be
simplified (for example "why not just run as admin?").

## Decision

We record significant decisions as numbered Markdown ADRs in `docs/adr`, using
`0000-template.md`. An ADR is proposed in the pull request that implements it, and it is
superseded rather than edited once accepted.

## Consequences

Decisions are reviewable and searchable. A small amount of writing accompanies big changes.
