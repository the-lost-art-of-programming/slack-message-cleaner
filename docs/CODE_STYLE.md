# Rust source conventions

Follow Praxis conventions in [`AGENTS.md`](../AGENTS.md) and [`docs/CONVENTIONS.md`](CONVENTIONS.md).

Use three-line, 77-dash comment separators around coherent sections:

```rust
// ---------------------------------------------------------------------------
// Domain logic
// ---------------------------------------------------------------------------
```

Use intentional whitespace, explanatory comments for non-obvious safety invariants, complete public Rustdoc with `# Errors` for fallible methods, and tested `# Examples` for pure APIs. Keep generated JavaScript limited to build outputs and retain the Rust/WebAssembly Working Group's Gloo dependencies.
