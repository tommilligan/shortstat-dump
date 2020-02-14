# Shortstat Parser

Dump short stats for commits over project history.

Run with:

```
cargo run -- --git-dir ~/reinfer/platform --patch --no-max-parents > shortstats.jsonl
```

## Output format

```json
{ "f": 2, "i": 10, "d": 0 }
```

Terse output of:

- `f`: files changed
- `i`: insertions
- `d`: deletions
