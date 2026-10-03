# Loaded script definitions

`loaded-scripts` builds an immutable catalogue from the winning source records
in an explicit inspection order. It reads original files with write-denying
handles and strictly validates each requested body. The catalogue keeps one
shared decoded record allocation for its authored scripts; borrowed names and
compiled bodies remain valid after the temporary record store is dropped.

```powershell
.\target\release\fallout.exe loaded-scripts `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --comparison-bundle local/loaded-scripts.bin `
  --output local/loaded-scripts.json
```

The supplied order is a reproducible inspection input. It has not been measured
as the original game's effective order. A deleted winning record does not fall
back to an older script. The CLI publishes the complete report before returning
1 for source metadata/ownership findings. Three original stale reference counts
remain diagnostic findings.

A `ScriptKey` combines the canonical profile/origin/local record identity with
the exact SCHR marker offset in the decoded record. Unrelated plugin reordering
does not change that key. Arbitrary record edits can change the marker. A handle
also carries a source version digest; lookup rejects a handle from an older
source version even when its key is still present.

Version digest format `FRSCRV01` starts with those eight ASCII bytes. Text fields
use a little-endian u32 byte length and exact UTF-8 bytes. Fields follow this order:
profile text, normalized origin text, local u32, SCHR offset u32, source name text,
source SHA-256 hex text, original record offset u64, original flags u32, decoded
record SHA-256 hex text, script metadata SHA-256 hex text, compiled-presence u8,
and, when present, compiled SHA-256 hex text and compiled byte count u64. Hashing
the entire source plugin also invalidates handles when an edit is outside that
script; conservative invalidation is deliberate.

Declarations retain authored order, sparse indices, duplicate indices, raw type
bytes and name hashes. Runtime access borrows the exact SCVR bytes, including its
terminal NUL. Index lookup returns the first authored match, following the pinned
xNVSE source model. This does not certify retail variable values or type inference.
Encoded references are one-based; zero never selects the first entry.

SCRO references resolve in the owning source's master namespace, then select the
whole-record winning header. Defined, deleted, missing, null and hardcoded engine
references are distinct states. The original corpus's hardcoded PlayerRef is a
runtime dependency, not an invented plugin record. SCRV associates a declaration
with a dynamic reference; its value is not available in a definition catalogue.

SCPT, QUST log entries and INFO begin/end owners use the already compared source
schema. PACK, TERM, PERK and placed-reference owner roles remain explicitly
unverified. A definition never represents a live event list, script instance,
local variable value, command invocation or scheduler entry.

The original C++ comparison tool rebuilds headers, source hashes, namespaces and
winners directly from the original locked plugins. It independently compares
every loaded unit, declaration, ordered reference, owner and version digest.
Uncompressed record payloads are compared directly with their original bytes.
For compressed bodies it checks the original decoded-size prefix; decompression
is still Rust-owned and is not independently certified by this comparison.

Completeness uses checkpoint 17's immutable, bound full table report and raw local
bundle. The native reader independently filters that full scan through its own
winning headers and hashes every surviving unit's identity/source/metadata.
This catches omissions from the new catalogue instead of comparing only its
selected subset. Moving the original header reader into a shared offline helper
also triggers a fresh checkpoint 23 header/membership/cache regression.

`FRCAT001` is a local-only decoded comparison bundle: eight-byte magic followed
by source index u8, kind four bytes, raw FormID u32, flags u32, original file offset
u64, decoded payload length u32 and that payload for each retained winning record.
`FRCOVER1` carries source index u8 followed by checkpoint 17's original record
tuple: kind four bytes, raw FormID u32, original file offset u64, decoded payload
length u32 and payload. Source indices name the explicit inspection order.
Both formats are bounded at 256 MiB. Raw bundles and source text stay local;
published receipts contain counts, identities, findings and hashes.

Execution, command effects, live event-list selection, timing, record-specific
retail merges and gameplay acceptance remain open.
