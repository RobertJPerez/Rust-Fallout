# Authored source probe fixtures

`fallout script-fixture` writes one bounded script carrier, exact source manifest,
explicit engineering initializers, and trace shape to a fresh private directory.
It never executes the original game or generates an original expected value.

```powershell
fallout script-fixture --install <read-only-retail-install> `
  --request <fixture-request.json> --profile-receipt <receipt.json> `
  --destination <new-private-directory>
```

The strict version-1 request contains `purpose` (`assignment` or `conversion`), a
nonzero canonical `campaign` byte array and `activation`, and `input` and
`destination_before` canonical Number values. For example,
`{"kind":"number","bits":9223372036854775808}` supplies exact negative-zero
bits. Inputs must be finite. Branch/native templates, unknown fields, NaNs and
infinities are refused. Request bytes are limited to 4 MiB; generated plugin bytes
to 4096 and profile-receipt bytes to 1 MiB.

The generated `RustFalloutProbe.esm` contains one SCPT at local ID 0x800, two
declared locals, no masters, no references and no source text. SCDA is 30 bytes:
script-name preamble at 0, Begin at 4, set-to own-local read at 14 and End at 26.
Assignment declares both slots Float; conversion declares the destination
Integer. The writer reuses the pinned layout evidence documented by
`script_units` and the existing source plans; it introduces no decoder. Existing
plugin and script-unit decoders check generated framing, and the CLI reloads
through RecordStore, Catalogue and PreparedSources before validating the manifest.
Truncation checks require a complete decoded script, including the enclosing group.

Outputs include `authored-source-copy/Data/RustFalloutProbe.esm`, `order.json`,
`manifest.json`, `copy-request.json`, profile receipt bytes and `receipt.json`.
The private source copy also contains original executable metadata for the
existing descriptor reader. Those bytes are never launched. The final receipt
records hashes, exact definition/version, full prepared-source cohort, winning
content digest, SCDA digest and expected *shape*. A failed/interrupted generation
has no final receipt. Existing destinations and files are never overwritten;
destinations within the supplied installation are refused.

The emitted order is a standalone structural decoder profile. Retail plugin
loading, activation and variable initialization remain unverified. An isolated
original recorder using a different load order must bind its actual source cohort
and receipt before capture comparison. The supplied receipt is hashed provenance,
not authentication or an isolation grant.

The generated assignment bundle can feed `script-trace --replacement-copy` through
the canonical engineering adapter. Conversion remains unsupported with unchanged
result state. The script-name preamble receives no assumed retail meaning; the
engineering adapter explicitly selects only the Begin/Assignment/End window.
Original capture absence keeps comparison blocked even after a successful copy.
`original_expected_output_generated`, `retail_loading_verified`, faithful execution
and gameplay acceptance remain false.
