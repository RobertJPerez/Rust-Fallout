# Physical condition source owners

`fallout-data::condition_operands::prepare_record_with_owners` admits one
winning, nondeleted record through the same strict source preparation as
`prepare_record`. It reuses that decoded body and the existing narrative decoder
for QUST and INFO. `PreparedOwnerRecord` provides immutable condition and owner
views; it holds no runtime state.

QUST conditions retain their physical quest, log-entry or objective-target source
container. Repeated stage/objective keys remain distinct sections identified by
marker offset and parent section index. A CTDA without an authored container
keeps `owner_section: null` and its finding. INFO conditions retain the INFO root
owner, including conditions after response and script markers. Other record
kinds have `unmapped_record_kind`, null site owners and no invented root list.

Every owner site names the prepared CTDA index and decoded field-header offset.
The adapter checks each mapped site's offset and exact CTDA bytes against the
narrative view. Sections and findings transfer from that view; its existing
canonical field digest is retained as `narrative_fields_sha256`. The underlying
condition view retains its complete source receipts, record identity, raw words,
OR flag, subject selector and descriptor findings.

`source_lists` collects physical membership by section, in first-member order.
It establishes neither AND/OR evaluation nor a default subject. Orphans remain
outside every list. `evaluation_ready`, `group_evaluation_verified` and
`default_subjects_applied` are always false. Descriptor binding and independent
source decoding establish no original-runtime command results or numeric rules.

The default owner limits are 65,536 sections, 65,536 findings and 128 MiB of
compact serialized owner metadata. Section/finding admission is enforced by the
existing narrative decoder. Borrowed section/finding metadata is byte-counted
before transfer; each site, list and index is counted before retention. Final
serialization must match the admitted byte count. These bounds describe admitted
source/report work rather than total process heap. Stored/decoded record, field,
condition and condition-byte limits remain those of `RecordLimits`.

The existing `condition-dependencies` inspector is the consumer:

```powershell
fallout condition-dependencies --install <installation> --load-order <order.json> --include-source-owners --output <new-report.json>
```

The flag emits schema 2, with per-record `source_owners`, aggregate
`source_owner_counts` and the false evaluation/default-subject flags. The default
command retains schema 1 and its previous bytes. The inspector admits the whole
record, including owner metadata, before materializing the additional JSON tree
or adding it to the retained aggregate. Existing limits remain 262,144 candidate
records, 512 MiB decoded candidate bodies, one million conditions and 128 MiB
compact retained record rows. Findings, orphan conditions and unmapped conditions
produce a diagnostic report followed by exit 1. Malformed owner fields reject
the optional inspection before report publication.

Focused tests cover repeated keys, physical marker offsets, root INFO ownership,
orphans, unmapped record kinds, exact byte boundaries, section/finding limits and
unchanged condition projections. Private authored fixtures compare section
tables, findings and field digests with the independently compiled native
narrative reader, as well as explicit authored CTDA membership. The frozen prior
inspector supplies the default byte comparison; cold/warm metadata caches retain
the same source projection. Changed owners, memberships, markers, parents,
digests, raw OR flags and evaluation claims are rejected by that comparison.
These checks establish source structure only; original runtime behavior remains
unmeasured.
