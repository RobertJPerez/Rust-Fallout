# Exact integral source literal assignment

VM30 stages one own numeric assignment from the compiled source of the existing
saved journal head. The explicit policy is
`EngineeringExactIntegralDecimal`. It uses existing source plans, bound
declarations and canonical opaque event changes. Faithful refuses because
original parsing, conversion and event lifetime remain unverified.

`literal_assignment::stage` takes the current World, PreparedSources, Content,
Selection `{ sequence, intent }` and Limits. It requires exactly BEGIN, one own
numeric assignment and END; the expression must contain one existing Number
token. Exactly one destination binding must identify the current own numeric
declaration and its literal source offset. Extra statements, operators, foreign
destinations, nonnumeric declarations and other expression tokens refuse.
No source report, mutable token stream or deserialized trace grants authority.

The literal byte bound is checked before conversion. The engineering grammar is
ASCII integral decimal digits, with a sign only if retained inside the Number
token. The current decoder recognizes initial signs as separate operators;
actual signed expressions and unary tilde refuse. No sign is inferred from a
neighboring token. Fractions and exponent spellings refuse even when their
mathematical value would be integral.

Checked digit accumulation produces a u128 magnitude. Overflow refuses. For a
nonzero value, its highest set bit gives the exponent. Values with more than 53
significant binary digits must have zero in every discarded bit. The exact
53-bit significand and biased exponent directly produce the binary64 word;
there is no floating-point parse, integer-to-float cast or rounding policy.
Zero uses the observed token sign. This bounded arithmetic domain deliberately
does not admit every finite binary64 integer.

The adapter captures the existing bounded EventObservation for the destination,
including its previous stored value, source bytes, bindings, owner, pending event
and both contexts. It reserves twice the projection's variable payload plus
the source/decoder digest bytes before retaining the canonical stage and extra
diagnostics. The complete compact trace must fit before stage creation.
`StagedLiteral` privately owns the existing StagedEventChanges; its trace is
historical diagnostic data. Only `commit` applies one Number value and
acknowledges the existing head atomically. Existing campaign/cohort/definition,
revision, epoch, head and counter checks refuse before effects. No reference,
instance or pending event is created, and clocks are unchanged.

The actual saved consumer is:

```text
event-operands --install INSTALL --load-order ORDER
  --snapshot-literal-assignment-request REQUEST --snapshot-input INPUT
  --snapshot-output RESULT [--output REPORT]
```

Strict request schema 1 requires the sequence, exact expected owner, intent and
all limits. The accepted engineering intent string is
`engineering_exact_integral_decimal`; generic `engineering` does not select it.
Inputs are current snapshot schema 4. The private restored World is discarded
after any refusal or failure. Successful results must encode within their limit,
cold-restore and equal the entire result before publication. Complete pretty
reports, including their final newline, are admitted before fresh output creation.
Existing protected-tree, distinct-path and create_new helpers are reused.
An interrupted filesystem write remains an incomplete artifact.

Defaults are 4096 selected-event instructions, one operand use, 65539 statement
bytes, 128 literal bytes, 1 MiB observed source bytes, 65536 observation rows,
1 MiB observed variable payload, 262144 complete-definition binding uses,
3 MiB stage variable reservation and 2 MiB compact trace. Preparation of the
selected whole definition separately limits instructions, operand uses, tokens
and decoded owning-record bytes. The ordinary catalogue/content loaders retain
their existing fixed ceilings. Request/input/result/report ceilings are
16 KiB/64 MiB/64 MiB/8 MiB. Caller limits can only tighten ceilings.
Logical payload/work bounds do not claim allocator accounting.

Independent expected words cover zero, leading zeros, 1, 10, `2^53-1`, `2^53`,
`2^53+2`, `2^64-2048`, `2^64` and `2^127`. A separate Python Fraction receipt
decodes the specified IEEE words and compares arbitrary-precision integers,
without using production arithmetic or a floating-point parser. Runtime checks
cover every exact creation bound and one-under refusal, inexact integers,
u128 overflow, unsupported spellings/shape, stale epochs/revisions/head and
revision exhaustion. Entire expected and cold snapshots preserve the next event,
other owners, NaN/signed-zero locals, distinct context roles, inventory with
opaque/NaN condition data and pose signed-zero/subnormal/enable state.

Source layout evidence is pinned xNVSE
`0ccd23ad885ddae533c1790a3fc56cd073e38de3`, with both ScriptAnalyzer files read
completely. Its numeric reader uses strtod, and its expression reader checks
operators before numeric fallback. This is layout evidence, not original
rounding/conversion acceptance. No upstream implementation is copied or linked.
Original installations, saves/settings and research pins remain untouched.
Source decoding and engineering checks do not prove gameplay.

Three runtime tests and 104 actual saved literal CLI cases pass, including every
one of the 16 runtime/preparation/result/report bounds exactly and one under,
all ceiling refusals, required/unknown/duplicate/malformed/oversized requests,
explicit owner/head/policy errors, revision exhaustion and stale source/schema.
The fixture assigns own integer declaration 90 from source bytes at SCDA 19..35,
producing bits `0x4340000000000001`, then acknowledges sequence 1 exactly once.
Journal sequence `9007199254741099` stays an exact typed integer. Fresh,
protected, existing and aliased outputs are exercised through the real process.
The frozen replacement producer SHA-256 is
`456504c54a6a5ea4ce1c40b519aaec9925b114db1abd787fc35150feac359423`.
Another 452 actual CLI cases pass for existing event requests, reference and
quest boot, reference/numeric/foreign copy and native observation. Runtime and
CLI Clippy, the CLI build and formatting checks pass. Initial test ownership,
rejected-cache expectation and test-lint/slice-inference failures remain in
separate local receipts. Only the declared old numeric-copy optional path was
boxed with its identical OS parser for the historical monolithic enum; default
CLI behavior and its existing cases remain unchanged. Integration relocates
only the new thin command/dispatch hunks to the current runtime command module.
