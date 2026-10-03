# Implementation checkpoint 30: foreign live-local access

Foreign variables now resolve through the target owner's current live script
instance. Typed reads and atomic writes use that instance's declaration bank;
static attachments and the caller's locals do not replace a missing live bank.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 231 passing tests and Clippy with warnings denied |
| Winning form index | 628,464 classifications agree with independent C++ source headers |
| Foreign requests | All 25,549 identities agree with independent static operand coverage |
| Without supplied lists | 20,542 missing quest lists / 5,007 unregistered placed references |
| Explicit test lists | 21,872 values resolve / 3,677 indices absent in the chosen test banks |
| Canonical state | 8,296 instances; 4,243,652-byte snapshot |
| Restore | Every full lookup result agrees after same-process and cold native restoration |
| Runtime tests | Nine cover current lists, typed contexts, atomic writes, namespaces, winners and changed content |
| Fresh regressions | Native saves, compiled schemas, quest attachments, loaded/header/cache and operand/table/native/expression/descriptor comparisons |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The engineering snapshot SHA-256 is
`58d514ecce938467508d1588279722be2baa65ebaf12aa2ff474f8923cecddb2`.
The digest of full bound lookup outcomes is
`1f4732dae072155e92375107c90bd5ef04304dad03e36b8abb07a4358d97fa21`.
Missing indices in the disclosed placed template remain failures. No original
live lists, values, initialization, execution or scheduling were captured.

The source snapshot covers 232 source/tooling files at implementation
revision `57dc0a7fcbac3dda39b282c6754c3d649b878344`. Its digest is
`b71245601895b4fcac9d513c18cbdc56225948621eb786f49551a7af8a7fa4e5`. The fresh proof is
`local/foreign-contexts-30-verified`.

M1 remains unfinished and no gameplay scenario is accepted. Next: immutable
inventory inputs, primitive queries and bounded observable execution components.

See [foreign live context](../docs/foreign-live-context.md),
[the scoped receipt](checkpoint-30-foreign-contexts.json) and
[verification](checkpoint-30-verification.json).
