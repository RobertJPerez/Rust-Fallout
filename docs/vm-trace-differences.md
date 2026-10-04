# Bounded trace difference display

`fallout script-trace` adds a report-only `difference_context` after the existing
comparator validates and classifies supplied data. The comparator, input schemas,
provenance checks and exit behavior are unchanged. A matched comparison has null
context. This field is a diagnostic, never an execution or recorder authority.

The first difference includes the producer, source step and precise field path.
Manifest, original and replacement sites show event ordinal/ID, Begin and operation
SCDA offsets, operation, normalized caller roles and operand count. Recorded sites
explicitly say whether their input matches the selected manifest. An altered
recorded offset/caller is displayed as supplied; it is not relabeled as a validated
source site. The report already retains the exact manifest definition/version,
source cohort and compiled-source digest.

Output differences select the first differing return, successor, ordered local
write or error. Input differences select the first changed ordinal, event, offset,
operation, caller role, activation, operand word or item. Only that selected pair
is retained: exact hexadecimal word format/bits, write index and ordering position
remain visible without cloning an operand or effect vector. Expected input is
identified as `manifest`; expected output is identified as `original`.

Missing/extra steps report counts and the first missing/extra index. A missing
recorded site stays null. A global identity, producer, completion or missing-capture
failure receives no guessed source index. Identity failures identify the exact
changed receipt/definition field. A replacement executable identity failure
expresses its required inequality to the original executable; it does not invent
an expected external producer digest.

Variable text and form names are bounded before cloning: at most 64 UTF-8 prefix
bytes, original byte length and whole-field SHA-256, with a truncation marker.
Prefixes end at a character boundary. Error differences also identify the first
differing UTF-8 byte when both sides have text. Compact diagnostic JSON is bounded
to 8192 bytes; this is not a limit on the entire existing report or its pretty
printing. Input byte, word, step and write admission limits still apply first.

The executable regression compares a real canonical engineering copy against a
disclosed synthetic original-labelled observation, then checks altered caller,
source offset, raw operand, event ordinal, missing/extra step, completion, error,
effect ordering, return and successor cases. Synthetic test fields are not retail
observations. Independent original capture and gameplay acceptance remain absent;
transport authentication, faithful execution and retail execution stay false.
