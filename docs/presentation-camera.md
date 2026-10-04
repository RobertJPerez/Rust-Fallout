# Reproducible inspection cameras

`--camera-receipt PATH` accompanies a successful `--capture` and records the
actual perspective camera after input controls. `--camera-restore REQUEST`
restores that record once the source scene and actual render viewport are ready.
These options apply to model, interior and terrain inspection. They use fresh
output files outside the installation, native repository, explicit model source
directory and restore request directory. A camera record is separate from the
immutable initial source report.

The strict version 1 JSON preserves the actual translation/quaternion binary32
words, perspective FOV/aspect/near/far/near-plane words, physical viewport and
logical viewport words. It also records the reversible source position/direction
binary64 words and source-origin words. When supplied, the initial source camera
position and target retain their original binary64 words even after controls
move the actual view. Restoration uses the retained renderer words; it never
recomputes orientation from a rounded source target or normalizes a quaternion.

Source identity hashes the existing source observations, every placement and
omission, and canonical view bindings through a bounded 64 MiB streaming writer.
Save-slot generations and residency progress are excluded; publishing an unchanged
Save does not change camera identity. The record binds that identity, source origin, current
scene epoch and canonical revision when present. Changed identity/origin/epoch/
revision or viewport refuses before moving the camera. Input and complete output
including newline each have a 4 KiB ceiling. Nonfinite coordinates, source-origin
precision loss, nonunit/singular/vertical rotation, invalid projection, unsupported
orthographic/custom/oblique projection and unknown or absent fields are refused.

The renderer updates perspective aspect from the actual logical viewport; the
record captures that value along with physical dimensions instead of inferring a
startup default. Viewport readiness has a 30-second deadline after scene admission.
Capture freezes inspection camera input while its opted-in
screenshot is pending, retains the requested camera words and verifies them again
at readback. Close remains available. Changed camera/source readback produces no
successful camera receipt. PNG and receipt success require the actual writes and
sync operations; failed artifacts remain explicit.

This is an engineering camera consumer. It does not establish original camera
defaults, physical input feel, projection parity, player movement or gameplay.
Automatic source replacement and other projection consumers remain separate work.
