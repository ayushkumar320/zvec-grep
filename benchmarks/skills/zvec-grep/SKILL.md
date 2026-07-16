---
name: zvec-grep
description: Use the prepared zvec-grep index for focused repository search instead of grep or rg.
---

# zvec-grep

The repository index is already prepared. Do not run `zg --index`,
`zg --index --rebuild`, or `zg --disable-index`, and do not change its embedding
configuration.

Use `zg` for repository search instead of calling `grep` or `rg` directly.
Start exploratory searches with a short natural-language query:

```sh
zg "where request authentication is validated"
```

Add `--fts` for exact symbols, flags, errors, or other anchors. Use path filters
early to keep results focused:

```sh
zg "authentication flow" --fts "AuthService" --include "src/**" --limit 5
```

Broad candidate scans should use `--preview none`. Use `--preview short` while
narrowing and `--preview full` only for a few final results:

```sh
zg "cache invalidation" --limit 20 --preview none
zg "cache invalidation" --limit 5 --preview short
```

Use `zg --rg` for exhaustive literal or regular-expression verification:

```sh
zg --rg -F "ExactSymbol" src
zg --rg -i -C 2 -g "*.py" "error pattern" .
```

Do not use zvec-grep when the task does not require searching repository files.
