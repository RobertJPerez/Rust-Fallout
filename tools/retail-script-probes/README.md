# Retail script probe transport boundary

`runner.rs` is compiled into the Rust `fallout script-trace` command through one
declared CLI module/dispatch hunk. It uses the existing catalogue, prepared source
plans and `fallout-runtime::execution::trace`; it contains no native retail calls
or second bytecode parser.

See [script-measurements.md](../../docs/script-measurements.md) for command,
schema, limits, negative cases and acceptance boundaries. Coordinator-owned
`tools/retail-profile` supplies isolated launch/capture receipts. An unavailable
original or replacement capture must remain an explicit blocked comparison.

`--replacement-copy` produces real engineering own-local observations through
the existing runtime stage/commit APIs. See
[vm-copy-probes.md](../../docs/vm-copy-probes.md). This producer writes no
original expectation and never launches the original engine.

`fallout script-fixture` authors bounded assignment/conversion source carriers,
exact manifests and explicit inputs through `fixtures.rs`. Existing decoders and
prepared plans round-trip the output. See [vm-probe-fixtures.md](../../docs/vm-probe-fixtures.md).
It emits trace shape only; retail loading, activation and original output remain
unverified.
