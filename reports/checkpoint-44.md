# Implementation checkpoint 44: actor and skin source lanes

Exact NPC_/CREA scalar sources and three skin source blocks now have fresh
independent comparisons at one frozen integrated revision. The full runtime/source
regression passed at that same revision. Source decoding does not initialize
actors, evaluate skinning or establish gameplay parity.

| Evidence | Result |
| --- | --- |
| Workspace | 359 Rust tests, formatting, Clippy warnings denied; 5 publication checks |
| Proof boundary checks | 3; changed valid-length native digest rejected by runner |
| Actor source scope | 6,455 winners, 204,929 physical fields, 17,130 scalar occurrences |
| Actor cold/warm/reordered | Exact definitions, fields, winner/cohort hashes; zero scalar findings |
| Altered fatigue | Rejected for differing source definitions |
| Skin authored scope | 48 fixtures across 12 stream revisions; 24 altered reports rejected |
| Skin sampled scope | 32 original stream 34 files, 500 blocks, 250 owners |
| Remaining skin dependencies | 250 partition payload dependencies; runtime_ready=false |
| Runtime/source regression | Fresh checkpoint 43 verification surface; earlier publication preserved |
| Installation | 464 files / 9,907,238,722 bytes still match baseline |

The proof binds 318 source/tooling files at `9e23c2e4ba4418edebfdb9e419261b20861ab94f`,
digest `65cd8fd84bba9c45705a8b8016a869d34cf7f2bf6e402d0475040474866b9a93`. The outer runner additionally binds both new actual
native binaries and the inspection profile before and after the run. Raw reports,
sampled assets and logs remain in `local/source-lanes-44-verified` and
`local/source-lanes-44-regression`.

The skin scope admits 12 authored stream revisions, while the selected original
samples are stream 34. Sampling is not a whole-corpus proof; first-person and GRA
coverage remain open. Authored raw presence/count bytes are independently retained
around the pinned reader's normalization. The changed-digest negative belongs to
the runner's binary guard, not standalone CLI cryptographic verification.

M1 remains the first unmet brief milestone. There are zero accepted gameplay
scenarios. Actor initialization/inheritance, partition/bone evaluation, animation,
VM execution/native effects, original activation order and observed retail
behavior remain open. The fourth agent prepares reviewed integration candidates;
the primary continues scripting and promotes tested histories to main.

See [source lanes](checkpoint-44-source-lanes.json),
[verification](checkpoint-44-verification.json),
[source snapshot](checkpoint-44-source-snapshot.json) and
[remaining gates](../NEXT_STEPS.md).
