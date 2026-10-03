# Vanilla command and event descriptor inspection

The offline Rust inspector now reads command signatures directly from the
fingerprinted New Vegas executable. It does not start the original process,
load its code or call a native handler. The result is evidence for operand and
registry work, not an implementation of the listed commands.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
target/release/fallout.exe command-catalogue --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --output local/catalogue-new.json
```

The supported source is the authorized local `FalloutNV.exe`, version 1.4.0.525,
16,549,704 bytes, SHA-256
`3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d`.
A different executable digest rejects before entering the descriptor layout.
The same displayed version is not sufficient evidence for different executable
bytes. Extension commands and other distributions need their own evidence.

## Fields and provenance

The pinned [xNVSE CommandTable.h/.cpp](https://github.com/xNVSE/NVSE/tree/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse)
declare 40-byte command descriptors and 12-byte parameter descriptors for the
32-bit original process. `CommandTable::Read` uses an exclusive end pointer;
`Init`/`Add` assign the script-command range from `0x1000`. `GameScript.cpp`
declares the event and statement tables. These source addresses are only inputs
to offline file lookup. Their values never become callable runtime pointers.
The complete CommandTable.h was read; inspected source ranges and pins are in
[sources.lock.json](../sources.lock.json).

[Microsoft's PE specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
defines the DOS/PE headers, preferred image base and section mapping used by the
reader. The Rust tool checks PE32/i386 framing, initialized extents and overlapping
sections. It rejects uninitialized addresses, crossing reads, unterminated names,
invalid pointers and allocation limits. This is a bounded metadata view, not a
general executable loader or a complete PE validator.

Each row retains the descriptor's source file offset, table index, assigned ID,
stored opcode, long/short name, parent word, flags and parameter metadata.
Parameter type IDs and optional words remain their authored integers. Handler
presence is recorded as a boolean; a source handler's existence says nothing
about replacement-runtime support. Return values, mutations, errors and complete
caller rules remain unknown. No original help strings or executable payloads
are published with the receipts.

The exact source contains 640 script-command descriptors, 38 event descriptors,
16 statement descriptors and 638 parameter descriptor occurrences. Stored IDs
agree with their table positions. The statement table includes deprecated forms
which are absent from the current corpus; this inspection does not add execution
support for them.

## Independent comparison and content links

`tools/command-oracle` is an original offline C++ tool using Windows PE header
declarations and CNG hashing. It reads the executable directly, maps sections
independently and compares every descriptor field with Rust. Unlike checkpoint
15's SCDA header oracle, this comparison does not begin at Rust-extracted bytes.
Both readers reject a changed executable copy. Original installed files are
retained and hashed against the baseline.

All 217 observed top-level command IDs and 33 observed event IDs resolve to these
descriptors. Expression calls are still outside the count. The prior full census
contains 148 condition-function IDs across 80,627 occurrences. For this vanilla
profile, the tooling explicitly binds each condition ID to its script descriptor
using the declared `0x1000` command base and retains both IDs in the receipt.
All 148 bindings have an original condition handler. The join is metadata only:
conditions are not evaluated, and identifiers are not treated as interchangeable.

The command and event names now make coverage gaps concrete. For example, authored
`SetStage` calls need a quest transition implementation and side effects;
`AddItem` needs real inventory-instance rules; `ShowMessage` needs the original
UI bindings. A descriptor catalogue cannot satisfy any of those requirements.

The next layers are script reference/local-variable binding, expression and
argument decoding, a typed native-command boundary and separately verified
behavior. xNVSE remains a source reference with an incomplete per-component
license audit. No upstream implementation is copied or linked into Rust.
