# Independent compressed record extraction

The `compressed-records` inspector hashes the exact stored extent and decoded
payload of every compressed record in the explicit plugin order. Stored extents
include the four-byte decoded-length prefix. Deleted records and superseded
definitions remain in this source scan. Uncompressed bodies other than TES4 stay
deferred; this report does not certify their fields.

```powershell
.\target\release\fallout.exe compressed-records `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --inspect-checksum-mismatches `
  --output local/compressed-records.json
```

Strict mode is the default. The diagnostic switch retains checksum findings and
returns 1 after writing its report. Structural corruption, surplus bytes, wrong
decoded lengths and resource-budget failures still abort. No original bytes are
rewritten and no diagnostic payload becomes eligible for runtime use.

The original offline C++ tool independently reads the original plugin headers,
master tables, record flags and compressed source bytes. It holds write-denying
source handles throughout inspection. It shares the previously verified original
header reader, but does not link the Rust decoder or another zlib implementation.
Its separate bit reader, canonical Huffman tables, length/distance expansion and
Adler calculation follow the format rules in
[RFC 1950](https://www.rfc-editor.org/rfc/rfc1950.html) and
[RFC 1951](https://www.rfc-editor.org/rfc/rfc1951.html). Both full RFC texts were
read; their sample code was not copied. This bounded offline tool rejects preset
dictionaries. It is evidence for this profile, not a general compliance certificate.

Every source name, source digest, compressed record identity, stored digest,
decoded digest, byte count and checksum finding must agree with Rust. Coverage
also matches the bound complete diagnostic census. Original header/winner digests
match checkpoint 25. Source and binary hashes bind the fresh proof to committed
implementation bytes; all installation files are compared with the baseline.

The known base-plugin LAND `00150FC0`, at file offset `0x0B0CFF04`, independently
decodes to 4,385 bytes with SHA-256
`ad04d26ea0186f5aa9cc1ee3a5a0172a413eda20ed3d4586738ebeb81d4ff8c2`.
The stored Adler value is 1,852,853,964; both calculations yield 1,816,153,796.
That agreement isolates the checksum finding. It does not establish how the
original game treats the record or justify a production compatibility exception.

Authored fixtures cover all fixed literal ranges, overlapping distance-one
matches, cross-block matches, the 32 KiB distance boundary, a single EOB symbol,
empty unused distance alphabets and dynamic zero-run repetition. Additional
deterministic fixtures come from the pinned Rust compression library at three
levels and lengths across the stored-block boundary. Both decoders must produce
the exact authored bytes and checksums. Malformed wrappers, trees, repeats,
symbols, matches, extents and bundles fail without publishing a report. A damaged
checksum fixture preserves its decoded digest in diagnostic evidence while the
Rust library rejects it.

`FRZLIB01` is a bounded local comparison bundle: eight-byte magic, then repeated
decoded-length u32, zlib-frame-length u32 and frame bytes. Integers are little
endian. The native tool permits at most 65,536 frames, a 256 MiB bundle, 64 MiB
per frame and 4 GiB total decoded fixture data. Its direct corpus mode uses the
existing `FRORDER1` order bundle. Raw original bytes and comparison bundles stay
under ignored `local/`; public receipts contain hashes, counts and findings.

Typed field interpretation, original effective load order, retail checksum
handling, script execution and gameplay acceptance remain separate unfinished work.
