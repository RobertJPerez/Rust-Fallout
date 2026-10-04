# Controller device events

Enable `bevy_gilrs` on the existing exact Bevy 0.19.1 dependency. Its normal
`DefaultPlugins` installs the device backend; presentation owns focus, reconnect,
dead-zone/action interpretation and context boundaries. No direct Gilrs dependency
or alternative mutable input authority is added.

The lock adds nine packages and only the Bevy internal gamepad dependency edge.
It changes no existing package version or checksum. The selected Windows backend
is Windows Gaming Input (`wgi`), with Gilrs 0.11.2 and core 0.6.8. XInput is absent;
enabling both backends is unsupported by the upstream conditional compilation.
All newly resolved declared MSRVs fit pinned Rust 1.99. Archive checksums, packaged
manifests/notices and the exact upstream notices were verified; see
[source identities](controller-dependencies.json).

Gilrs/core omit their root notices from the published archives, so the exact VCS
revision's MIT/Apache texts are retained under `docs/licenses`. The packaged
SDL controller mapping database has a separate Zlib notice retained there too.
The macOS-only objc2 dependency documents unresolved Apple SDK derived-work
licensing questions; that platform needs its own distribution review.

Windows Gaming Input needs a focused window for reliable device events. Synthetic
Bevy event tests establish adapter behavior, while actual device connect, input,
disconnect and reconnect need a windowed hardware check. Backend startup failure
must remain visible. Dependency audit and lock resolution alone do not establish
that hardware acceptance or original input parity.

Primary sources: [pinned Bevy manifest](https://raw.githubusercontent.com/bevyengine/bevy/v0.19.1/crates/bevy_gilrs/Cargo.toml),
[Gilrs platform notes](https://docs.rs/gilrs/0.11.2/gilrs/),
[exact Gilrs notices](https://gitlab.com/gilrs-project/gilrs/-/tree/07e286e24b046cf39e5c367daa2770b805a64692),
[objc2 license scope](https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md).
