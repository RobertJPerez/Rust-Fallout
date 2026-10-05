# Source audit

Exact revisions and inspected paths are in `sources.lock.json`. Rust runtime code
was authored for this adapter and uses the existing shared Rust library/backend.
No reference engine implementation was copied into it. Format constants and
structural facts were checked in primary readers/compiler output code.

The offline PEX oracle links Champollion's original Pex C++ sources. It is a
research executable, separate from Cargo and never called by the production CLI.
Its source/exporter and build recipe are available here; upstream source stays in
an ignored pinned checkout. Do not distribute that binary without satisfying its
upstream license. Forced standard header includes in CMake accommodate missing
transitive includes without editing the reference checkout.

The runtime archive backend is MIT/Apache-2.0 and retains its own dependency
metadata/licenses in Cargo's source distribution. Its mmap layer contains unsafe
platform code; this project's Rust forbids unsafe. Compression uses Rust zlib/LZ4
implementations. Platform memory mapping and system runtime calls are dependencies,
not native gameplay implementations. Rust/Cargo are pinned; Cargo.lock pins all
transitive versions/checksums. Shipping would need a full dependency notice bundle.

Only original synthetic test data is checked in. User-owned retail assets and PEX
extracts remain private under `local/`. The master brief is read externally and
is not copied into the repository. Census success and independent structural
agreement do not certify gameplay or campaign compatibility.

The VMAD oracle references an unmodified pinned Mutagen checkout (GPL-3.0-only)
from an isolated .NET harness. It is an offline validation tool, not a production
Cargo dependency. Its small identity adapter preserves the uint supplied by the
upstream decoder before normal master-slot normalization. No upstream C# parser
implementation is copied into Rust. .NET build output currently reports an
upstream SourceLink build-tool dependency advisory (Microsoft.Build.Tasks.Git
10.0.300); this remains confined to the research build and is not suppressed.
