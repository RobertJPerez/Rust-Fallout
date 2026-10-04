# Inspection camera and action boundaries

The model, interior and terrain inspection host consumes `input::Actions` in
`main::controls`. Source camera requests must produce a valid Bevy `Dir3` before
the renderer can receive them. Finite endpoints are insufficient: subtracting
opposite large coordinates or computing a direction's length can overflow. Zero,
near-zero, vertical, nonfinite and unrepresentable relative views are refused.
The validated direction is passed to `looking_to`, avoiding Bevy's fallback facing.

The input adapter runs after Bevy's input processing in `PreUpdate`. It owns
inspection contexts (`Orbit`, `Fly`, `Suspended`) and produces an immutable action
sample for the camera consumer. It does not mutate canonical runtime state.

| Inspection action | Keyboard/mouse | Controller |
| --- | --- | --- |
| Toggle orbit/fly | Tab | Select |
| Move / orbit yaw and pitch | WASD | Left stick |
| Vertical movement / orbit zoom | Q/E; wheel zoom | Left/right shoulder |
| Look | Arrows; hold right mouse button and drag | Right stick |
| Faster fly movement | Either Shift | Left stick click |
| Reset camera | R | North (Y/triangle) |
| Close inspector | Escape | Start |
| Save selected project-native repository | F5 | Unassigned |
| Continue from its strict current slot | F9 | Unassigned |

Keyboard and controller look use frame time. Mouse displacement is applied once
without frame-time multiplication. Line and pixel scroll events are converted
individually before addition, so mixed-unit frames cannot be interpreted entirely
using the final event's unit. Input values and combined movement are bounded;
analog movement magnitude survives the camera adapter. The radial stick deadzone
is an explicit inspection choice, not an original-game measurement.

Losing focus, changing context, connecting/replacing a controller or a connection
change on the same controller enters a boundary. Held buttons are quarantined
until release; held sticks must return to neutral. Keyboard quarantine survives
empty samples after focus loss. The adapter reads primary-window `KeyboardInput`
messages: a release in a stable focused context or a fresh non-repeat press can
leave quarantine. Repeats cannot arm quarantined movement or shortcuts. `KeyboardFocusLost`
also enters a boundary, so Bevy's delayed synthetic releases cannot arm a key
even if the window has already regained focus. A fresh press can restore input
after a physical release outside the application was not observed. Keyboard
shortcut edges use these fresh source messages rather than `ButtonInput` edges.
Transient mouse motion and scrolling at a context/focus boundary are discarded.
A mode-toggle frame cannot apply movement from its old context to its new camera.
The selected controller stays selected until disconnect; replacement selection
uses stable entity ordering within the current process. Unfocused and suspended
contexts produce no camera actions. The offscreen capture uses Suspended.

Headless regressions cover source camera overflow and declared source basis vectors,
focus recovery, context leakage, mouse drag ownership, analog magnitude, disconnect,
reconnect and malformed device values. An additional headless Bevy application
exercises real keyboard, focus, mixed scroll and controller messages through the
production schedule. The focus follow-up reproduces a held W becoming movement
after empty focus-regain samples and an auto-repeat. Additional actual-pipeline
checks cover shortcut repeats, delayed focus clearing, fresh presses after an
unobserved release and another window's messages. Focused tests, affected-package
Clippy with warnings denied, formatting and whitespace results belong to the
corresponding handoff; the original 14-test handoff remains immutable.
Exact commands, logs and hashes are recorded in the lane's local handoff receipt.

Physical controller discovery uses Bevy's `bevy_gilrs` feature. The reviewed
coordinator dependency enables the pinned Windows Gaming Input backend; its
manifest, lock and notices are recorded in the controller-input decision.
Injected Bevy controller events alone
do not establish working hardware input. The manual window check could not run:
the Computer Use native pipe was unavailable after its prescribed recovery.
Physical keyboard/mouse/controller feel,
rebinding/accessibility options, retail camera equivalence, player movement,
collision and original menu behavior remain separate validation or implementation
work. No gameplay or original presentation parity is accepted by these tests.
