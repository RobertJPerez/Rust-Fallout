# Preparing script sources once

`programs::PreparedSources` prepares an immutable script catalogue once. Pending
events then borrow its validated source plans instead of rebuilding control flow,
expression arenas and operand associations for each event. Live locals, reference
bindings, player identity and foreign event lists are still resolved on every
probe.

This is a source admission layer. It executes no instruction, changes no value,
consumes no journal entry and grants no native-command permission. The original
game's arithmetic, conversion, control effects and event lifecycle still need
independent behavior measurements.

The cache borrows an external immutable `loaded_scripts::Catalogue`. It owns
plans and shared rejection objects without self-referential storage, interior
mutation or a second live-state bank. Worlds with the same complete source cohort
can share it. Restoring a world does not restore transient instance handles or
require source preparation again. The cache is not part of the save format.

## Identity and admission

The source cohort uses the same canonical identity as `World`. It includes all
source receipts and winning definition identities, not just the selected script.
Every lookup validates the exact source handle. Identical compiled bodies with
different embedded owning tables retain separate plans and binding digests.

A separate decoder digest records the validated vanilla operator descriptors,
including precedence, and the ordered native signature parameters and conventions.
It identifies the structural decoder inputs; it does not certify retail semantics.
The caller chooses these inputs and the source policy when constructing the cache.

An absent SCDA field returns a shared `MissingBody` rejection. An authored empty
SCDA field goes through preparation and can yield an empty plan. Metadata findings
take precedence over an absent body. Other source failures remain stable shared
rejections; repeated lookups do not retry them or turn them into partial plans.

Construction checks definition and source-receipt counts before retaining the
cohort projection. It bounds signature parameters, attempted compiled bytes,
attempted owning-record bytes and retained instructions, expressions, tokens,
arena nodes and operand uses. Failed attempts consume source and record work
allowances too. Remaining aggregate limits reach the instruction, expression and
binding readers before their allocations. Expression arenas reuse the private
token stream decoded by the statement reader.

Exhausting a configured preparation allowance aborts the complete construction.
There is no partially admitted cache. Existing native/table readers also have
fixed admission limits: those diagnostic failures remain source rejections and
cannot produce a prepared plan. Retained counts and byte-work charges are not a
measurement of total heap use. Individual source preparation still uses the
existing owning-table and binding decoders; this layer avoids repeated preparation
across events, rather than claiming every physical byte is read only once.

## Event preparation and inspection

`World::prepare_event_with_sources` checks the complete cohort, current instance,
pending context, exact definition and authored BEGIN offset/event ID. The event
instruction allowance includes both delimiters. Repeated event IDs select their
own exact windows. `World::probe_event_operands_with_sources` uses that frame and
the same live resolution path as the existing fresh-preparation API.

The inspector offers an additive mode:

```powershell
.\target\release\fallout.exe event-operands `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles\nv-inspection-order.json `
  --prepared-sources --output local\prepared-operands.json
```

Default mode retains schema 1. Prepared mode adds schema 2 `prepared_sources`
counts and the source/decoder identities. Its events and live outcomes have the
same projection. The historical `attempted_source_bytes` field still counts bytes
per pending event; `prepared_sources.counts.attempted_source_bytes` records actual
once-per-definition preparation work across the whole catalogue. The inspector
writes findings and returns 1 while source or storage results remain unresolved.

Tests cover shared plan identity, absent and empty bodies, rejection reuse,
exact/one-less budgets, repeated windows, full-cohort changes, different embedded
tables, local/player/foreign-list changes and restoration without source rebuilding.
These checks establish source reuse and explicit engineering-state behavior.
They do not establish gameplay parity.

Private development preparation of the ten original inspection plugins checked
80,627 units: 14,479 preparation attempts produced 14,420 plans and retained 59
source rejections; 66,148 units had no SCDA field. Fresh and prepared cold/warm
operand reports agreed exactly for 2,160 pending entries and 54,498 selected uses.
A saved-fixture audit against digest-bound retained native source evidence passed
those uses and ten altered-evidence cases. This is development validation; fresh
integrated source/binary proof is still required before checkpoint publication.
