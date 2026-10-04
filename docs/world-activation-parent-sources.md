# Placed activation-parent source inputs

The owned `fallout placed-activation-sources --install ... --load-order ...`
command requires an explicit canonical `--reference Origin.esm:hex`; it accepts
an optional existing `--index-cache`. It emits raw prompt/hash, parent delay
words, all terminal resolution statuses and actual source witnesses. Exit zero
means the source closure was prepared, including truthful absent or unavailable
fields. A factory/root refusal emits a null request and source error with exit
one. Earlier strict index/master refusals return the existing error before a
report. Timer, activation, condition, actor/item/current CELL, prompt decoding,
default prompt and runtime/parity outputs stay false.

`world::activation::PlacedActivationSources::load(&mut RecordStore, &FormKey, Limits)`
prepares the winning live NV REFR/ACHR/ACRE requested by the caller. Its private
immutable owner provides a borrowed receipt, root, identity and ordered source
validation. Reports cannot construct a request.

The consumer retains an optional singleton XAPD byte, each physical XAPR occurrence
(parent raw u32 and exact IEEE-754 delay word), and optional singleton XATO bytes
including the final NUL and their SHA-256. Repeated parents and disc order survive.
Negative zero, subnormal, negative and NaN delay words remain u32 values; there is
no float conversion, duration, timer or activation result. A raw XAPD value outside
the editor BoolEnum is preserved without inventing an activation policy.

Each field carries both physical and logical ordinals, decoded header/payload
spans and complete framing, including a validated XXXX prefix. Physical framing
offsets refer to uncompressed records only. Compressed bodies retain their stored
offset/extent and decoded spans; they have no invented physical field offset.
The existing core placement decoder still reports these fields as unhandled.
It is preflighted for physical fields and possible map entries before allocation.

Parent targets use the existing canonical master/winner policy and the pinned
REFR/ACRE/ACHR/PGRE/PMIS/PBEA/PLYR header domain. Null, missing, deleted,
wrong-kind and reserved runtime-player-binding-unimplemented statuses remain
explicit. Actual winning headers are copied where available. No target body,
target placement, parent graph or activation callback is read or evaluated.

XATO reuses the existing strict supported string branch: exactly one final NUL,
no embedded NUL. Empty text is a single NUL. Non-UTF8 bytes are preserved and
hashed without encoding conversion or repair. Editor string display and sorted
array presentation do not establish source ordering or a runtime default.

Ten lowerable ceilings cover sources, copied record witnesses, physical fields,
parent occurrences, links, full prompt bytes, individual/aggregate source reads,
raw framing bytes and conservative copied metadata. Repeated header witnesses
are charged per occurrence. Prompt metadata includes both raw and framing copies.
Whole source-order/name/count/hash identity binds the request; changed cohort,
tainted bodies, duplicate singletons, unsupported widths and malformed strings
refuse the whole factory result. There is no partial request authority.

Independent literals test repeated unsorted parents, four unusual delay words,
non-UTF8 text, every target kind, source overrides, absent/unavailable fields,
XXXX/compression, strict refusal and exact/one-under admission. These checks
establish source preparation only. Runtime timing, conditions, actor/item state,
current CELL changes, default prompts and gameplay parity remain unavailable.

Pinned schema: xEdit commit `9fb016884bec138ea6c7b872cec831537d464c3e`,
complete FNV ACHR/ACRE/REFR declarations (activation parents at lines 3164,
3246 and 7719) and complete common string helpers. FNV definitions SHA-256:
`89b1420415b858e004429e89a828348a1df71f5c8b199d4ea66e02f0ca41b012`.
Common helpers SHA-256:
`e616b6546f6df74d88ec98bb870906db380c985963d011e4931c5726682b7d26`.

