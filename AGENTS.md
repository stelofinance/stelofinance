# AGENTS.md

The role of this file is to describe common mistakes that agents run into when working in this codebase.

If you ever run into something in the codebase that surprises you, please alert the developer working with you and add that case to this `AGENTS.md` file to help prevent future agents from having the same issue.

## HTML templates (`tylermmorton/tmpl`)
- The tmpl static analyzer only allows bare `{{if .Field}}` when `.Field` is a **bool**. For strings/slices/ints use comparison builtins with **two explicit args** (no pipe into `gt`/`eq`): `{{if ne .Name ""}}`, `{{if gt (len .Items) 0}}`, `{{if gt .Total 0}}`. Pipelines pass the prior result as the **last** arg, so `len .X | gt 0` is not how the analyzer expects `gt` to be called.
