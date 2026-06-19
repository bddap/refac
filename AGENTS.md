# refac — contributor notes

## Comments are code

A comment must **provably earn its place**: it survives only if it carries a
**WHY**, a **gotcha**, or a **constraint** a future reader would otherwise trip
over. Never restate what the code, a signature, or a type already says; never
narrate WHAT the next lines do; never leave development-history trivia ("changed
from…", "the API doesn't infer this"). **When in doubt, delete.** Comment density
is itself a cost — a wall of even-true remarks buries the few that matter and
makes the code harder to read.

Doc comments on crate-internal items get the same bar: keep one only for a
non-obvious WHY or when a macro consumes it (e.g. a `schemars` field doc that
becomes a model-facing schema description).

## Types

Prefer real types over `serde_json::Value` or stringly-typed data for anything
refac constructs or controls. The one sanctioned `Value` is a payload echoed back
to a provider verbatim for byte-fidelity (re-serializing would reorder fields) —
and that exception carries a WHY comment.

## Build & test

The toolchain is pinned via nix, not rustup. From a clone:

```bash
cargo test
cargo clippy --all-targets -- -D warnings   # must stay clean
```

Both must pass before requesting review.
