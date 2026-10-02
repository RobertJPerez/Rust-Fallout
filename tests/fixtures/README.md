# Fixture provenance

The parser and resolver fixtures are generated in `crates/fallout-data/tests/framing.rs`.
They are original byte streams made for this project: no copied ESM records, scripts,
models, textures, sounds, or upstream sample assets. Unit fixtures for paths, allocation
budgets, import identities, and interrupted publication live beside those modules.
`crates/fallout-data/tests/nif.rs` builds original NIF container fixtures in memory.
Cell membership, indexed reads, and typed-link fixtures extend `framing.rs`.

Retail comparisons read the user's installation and write metadata under `local/`.
That directory, tool downloads, and decoded asset caches are ignored by Git. A passing
synthetic test demonstrates its named case; it does not establish retail gameplay parity.
The nifly comparison driver consumes only locally decoded, digest-verified cache files;
no upstream NIF test models are used or redistributed.
