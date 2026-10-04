# Retail script probe transport boundary

`runner.rs` is compiled into the Rust `fallout script-trace` command through one
declared CLI module/dispatch hunk. It uses the existing catalogue, prepared source
plans and `fallout-runtime::execution::trace`; it contains no native retail calls
or second bytecode parser.

See [script-measurements.md](../../docs/script-measurements.md) for command,
schema, limits, negative cases and acceptance boundaries. Coordinator-owned
`tools/retail-profile` supplies isolated launch/capture receipts. An unavailable
original or replacement capture must remain an explicit blocked comparison.
