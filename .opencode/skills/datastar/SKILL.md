---
name: datastar
description: Datastar framework patterns, gotchas, and conventions
---

## When to use me

Load this skill when working on any HTML template that uses Datastar attributes, or any Go handler that returns SSE responses to the frontend via `datastar-go`.

## Critical rules

### HTML attribute keys are case-insensitive

Never use camelCase in a `data-*` attribute key. The DOM lowercases all attribute names.

- **WRONG**: `<el data-bind:accId>` — signal becomes `$accid`.
- **CORRECT**: `<el data-bind:acc-id>` — Datastar converts kebab-case key to camelCase signal `$accId`.
- **ALTERNATIVE**: `<el data-bind="accId">` — value syntax bypasses key parsing entirely.

This applies to every attribute that names a signal in its key.
