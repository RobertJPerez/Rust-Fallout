# Dependencies and reference scope

This workspace directly uses the user's existing `fallout-data` crate at a pinned
local revision. Its metadata does not grant a public distribution license; this
checkpoint is a local engineering deliverable. Keep its upstream dependency
notices when packaging shared code.

The archive dependency is `dream_archive`, copyright 2026 Dave Corley, licensed
under MIT OR Apache-2.0. Cargo retains its LICENSE-MIT and LICENSE-APACHE files in
the dependency checkout. Its transitive dependencies, plus serde, serde_json,
thiserror and tempfile, are pinned by Cargo.lock. No C++ gameplay engine is linked.
The archive backend uses memory mapping and its own platform implementation;
new workspace code forbids unsafe Rust.

xEdit (MPL-2.0) and esplugin (GPL-3.0) were consulted as format references; their
implementations are not copied into the production code. The separately built
`tools/plugin-oracle` executable links esplugin under GPL-3.0 for offline validation;
it is not a dependency of skyrim-prep. Any distribution of that executable must
carry the applicable GPL notices/source obligations. Its lockfile is separate.
Source revisions and inspected paths appear in sources.lock.json. No retail assets
are included.

`tools/vmad-oracle`, `tools/archive-oracle` and `tools/trace-oracle` are separate GPL-3.0-only validation
executables using Mutagen.Bethesda.Skyrim/Core 0.54.4, copyright Noggog and
contributors. Their NuGet metadata identifies repository revision
`0188012c607ce8bb283d2704400d37737f089134`; each has a dependency lockfile with
package content hashes. Mutagen's LICENSE.txt is GPL version 3. These tools and
their dependencies remain outside the Rust application; no Mutagen implementation
is copied into it. Distribution of the tools must include the applicable GPL
notices and corresponding source. NuGet packages retain their license metadata
under the private ignored package cache.
