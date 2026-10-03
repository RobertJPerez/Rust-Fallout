# Quest and dialogue source structure

The Rust loader now keeps QUST, INFO and DIAL fields in authored order and assigns
known fields to explicit source sections. This provides the content structure for
quest and dialogue systems; it does not run a quest or select a response.

```powershell
target/release/fallout.exe narrative --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/narrative-new.json
```

This corpus command writes a complete diagnostic report and returns 1 because
original ownership/header findings remain. Output paths must be new and outside
the installation. Without selective deferral, unrelated strict payload validation
still rejects the known LAND checksum defect.

The borrowed data module is `fallout_data::narrative`. It retains complete original
field slices, including unknown fields and text bytes. It does not assume UTF-8,
trim original text, insert editor defaults or apply xEdit after-load cleanup.
Reports contain byte counts and hashes instead of original dialogue or script text.

Each section uses its marker's decoded offset as its identity within a record.
Repeated stage/objective numbers or source FormIDs remain separate authored
entries. Parent indices refer only to earlier sections. Parsing is iterative and
bounded by field, section and diagnostic limits.

| Record | Authored structure preserved |
| --- | --- |
| QUST | General data/script link, conditions, signed stage indices, separate log entries and scripts, objective descriptions, targets and their conditions |
| INFO | Original flags, quest/previous-info/topic links, separate responses and text, INFO conditions, begin/end scripts, speaker, prompt and challenge fields |
| DIAL | General topic data/priority bits, added quest blocks, separate shared-info connections and signed indices, removed quest fields |

INFO conditions belong to the INFO record, not to its last response. Begin and
end scripts keep distinct section identities at SCHR markers; NEXT retains its
original empty marker. Quest scripts attach to their actual log entry. A separate
SCRI form link remains a link to a standalone script. Reference words retain their
source namespace until the loaded record store resolves them.

The measured source corpus contains:

| Check | Count |
| --- | --- |
| Quest / topic / INFO records | 642 / 21,560 / 28,933 |
| Original fields / sections | 593,244 / 176,792 |
| Responses | 37,447 |
| Embedded script units / compiled bodies | 59,225 / 9,709 |
| INFO / general quest / log-entry / target conditions | 69,332 / 322 / 26 / 1,736 |
| Unrecognized or unowned fields | 0 |
| Source findings | 20 across 17 records |

Eighteen shared-info connections in fifteen base-game DIAL records have no authored
QSTI parent. Their nodes keep an absent parent rather than a fabricated quest.
The pinned schema already notes examples `001287C6` and `000E9084`; the receipt
records the additional source sites. FalloutNV quests `0001E5CA` and `0002242A`
contain two-byte DATA headers. Flags/priority remain present; padding and delay
remain absent. Their retail loading/migration is unverified.

The pinned [xEdit FNV schemas](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
provide source layouts and section markers. Selected
[common definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsCommon.pas)
identify speaker bytes, removed-quest links and editor-only INFO-order fields.
The Rust runtime does not link or copy these implementations.

Original offline C++ independently reads the locally extracted decoded records and
compares typed field/owner digests, every section, script metadata, body hashes and
all findings. Record counts match the earlier full inventory; conditions and script
units match their prior source-bound receipts. The comparison starts after Rust
plugin extraction/decompression. It does not establish original runtime grouping,
dialogue ordering, script timing or behavior.

Original fixtures cover signed/repeated keys, short optional layouts, opaque text,
unknown fields, orphaned entries, distinct response/script owners, exact float
bits, malformed field lengths and resource limits. The independent fixture adds
four-byte quest headers and twenty-byte response layouts absent from this corpus.

Canonical winning record/script identities, INFO parent GRUP membership, actual
form existence, condition subject resolution, query behavior, dialogue selection,
quest state and retail gameplay acceptance remain unfinished.
