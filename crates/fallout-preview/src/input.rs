//! Inspection actions own focus and context; physical held state is never an
//! intent to enter a different context. These are inspection bindings, not
//! measured original-game controls or a canonical player movement implementation.
use bevy::{
    input::{
        ButtonState, InputSystems,
        gamepad::GamepadConnectionEvent,
        keyboard::{KeyboardFocusLost, KeyboardInput},
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    window::{PrimaryWindow, WindowFocused},
};
use std::{collections::HashSet, hash::Hash};

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Context {
    #[default]
    Orbit,
    Fly,
    Suspended,
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Actions {
    /// x: right, y: forward, z: up. Controller magnitude is retained.
    pub movement: Vec3,
    /// x: right, y: up, continuous keyboard/controller rate.
    pub look: Vec2,
    /// Right-drag displacement; the consumer must not multiply by frame time.
    pub pointer_look: Vec2,
    pub scroll: f32,
    pub fast: bool,
    pub reset: bool,
    pub toggle: bool,
    pub close: bool,
}

pub struct InspectionInputPlugin;

impl Plugin for InspectionInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Context>()
            .init_resource::<Actions>()
            .init_resource::<Boundary>()
            .add_systems(PreUpdate, collect_input.after(InputSystems));
    }
}

struct ButtonGate<T> {
    previous: HashSet<T>,
    blocked: HashSet<T>,
}

impl<T> Default for ButtonGate<T> {
    fn default() -> Self {
        Self {
            previous: HashSet::new(),
            blocked: HashSet::new(),
        }
    }
}

impl<T: Copy + Eq + Hash + Send + Sync + 'static> ButtonGate<T> {
    fn update(&mut self, physical: &ButtonInput<T>, boundary: bool, enabled: bool) {
        if boundary || !enabled {
            self.blocked.extend(self.previous.iter().copied());
            self.blocked.extend(physical.get_pressed().copied());
        } else {
            self.blocked
                .retain(|button| physical.pressed(*button) && !physical.just_released(*button));
        }
        self.previous = physical.get_pressed().copied().collect();
    }

    fn held(&self, physical: &ButtonInput<T>, button: T) -> bool {
        physical.pressed(button) && !self.blocked.contains(&button)
    }

    fn edge(&self, physical: &ButtonInput<T>, button: T) -> bool {
        self.held(physical, button) && physical.just_pressed(button)
    }
}

/// Raw primary-window transitions retain physical press/repeat information that
/// Bevy's shared ButtonInput discards. Focus clearing is never a physical release.
#[derive(Default)]
struct KeyTransitions {
    pressed: HashSet<KeyCode>,
    fresh: HashSet<KeyCode>,
    released: HashSet<KeyCode>,
}

impl KeyTransitions {
    fn record(&mut self, event: &KeyboardInput) {
        match event.state {
            ButtonState::Pressed => {
                self.pressed.insert(event.key_code);
                if !event.repeat {
                    self.fresh.insert(event.key_code);
                }
            }
            ButtonState::Released => {
                self.released.insert(event.key_code);
            }
        }
    }
}

impl ButtonGate<KeyCode> {
    fn update_keyboard(
        &mut self,
        physical: &ButtonInput<KeyCode>,
        transitions: &KeyTransitions,
        boundary: bool,
        enabled: bool,
    ) {
        if boundary || !enabled {
            self.blocked.extend(self.previous.iter().copied());
            self.blocked.extend(physical.get_pressed().copied());
            self.blocked.extend(transitions.pressed.iter().copied());
        } else {
            // Empty samples and auto-repeat cannot prove release. A release in
            // this stable focused context, or a fresh non-repeat physical press
            // after an unobserved out-of-focus release, can leave quarantine.
            for key in transitions.released.iter().chain(&transitions.fresh) {
                self.blocked.remove(key);
            }
        }
        self.previous = physical.get_pressed().copied().collect();
    }
}

#[derive(Resource, Default)]
struct Boundary {
    last: Option<(Context, bool)>,
    pad: Option<Entity>,
    keys: ButtonGate<KeyCode>,
    mouse: ButtonGate<MouseButton>,
    buttons: ButtonGate<GamepadButton>,
    blocked_sticks: [bool; 2],
}

struct Frame<'a> {
    context: Context,
    focused: bool,
    focus_changed: bool,
    pad_changed: bool,
    keys: &'a ButtonInput<KeyCode>,
    key_transitions: KeyTransitions,
    mouse: &'a ButtonInput<MouseButton>,
    pad: Option<(Entity, &'a Gamepad)>,
    motion: Vec2,
    scroll: f32,
}

/// Radial inspection deadzone. Sanitizing components before computing length
/// also prevents corrupt device events from contaminating camera transforms.
fn stick(value: Vec2) -> Vec2 {
    if !value.is_finite() {
        return Vec2::ZERO;
    }
    let value = value.clamp(Vec2::splat(-1.), Vec2::ONE);
    let length = value.length();
    const DEADZONE: f32 = 0.15;
    if length <= DEADZONE {
        return Vec2::ZERO;
    }
    value / length * ((length.min(1.) - DEADZONE) / (1. - DEADZONE))
}

impl Boundary {
    fn sample(&mut self, frame: Frame<'_>) -> Actions {
        let state = (frame.context, frame.focused);
        let boundary = self.last != Some(state) || frame.focus_changed;
        self.last = Some(state);
        let enabled = frame.focused && frame.context != Context::Suspended;
        self.keys
            .update_keyboard(frame.keys, &frame.key_transitions, boundary, enabled);
        self.mouse.update(frame.mouse, boundary, enabled);
        let pad_id = frame.pad.map(|(id, _)| id);
        let pad_boundary = boundary || frame.pad_changed || self.pad != pad_id;
        self.pad = pad_id;
        let mut result = Actions::default();
        if let Some((_, pad)) = frame.pad {
            self.buttons.update(pad.digital(), pad_boundary, enabled);
            let mut sticks = [stick(pad.left_stick()), stick(pad.right_stick())];
            for (index, value) in sticks.iter_mut().enumerate() {
                if pad_boundary || !enabled {
                    self.blocked_sticks[index] = true;
                } else if *value == Vec2::ZERO {
                    self.blocked_sticks[index] = false;
                }
                if self.blocked_sticks[index] {
                    *value = Vec2::ZERO;
                }
            }
            let held = |button| self.buttons.held(pad.digital(), button);
            let edge = |button| self.buttons.edge(pad.digital(), button);
            result.movement = Vec3::new(
                sticks[0].x,
                sticks[0].y,
                f32::from(u8::from(held(GamepadButton::RightTrigger)))
                    - f32::from(u8::from(held(GamepadButton::LeftTrigger))),
            );
            result.look = sticks[1];
            result.fast = held(GamepadButton::LeftThumb);
            result.reset = edge(GamepadButton::North);
            result.toggle = edge(GamepadButton::Select);
            result.close = edge(GamepadButton::Start);
        } else {
            self.buttons = ButtonGate::default();
            self.blocked_sticks = [true; 2];
        }
        if !enabled {
            return Actions::default();
        }
        let held = |key| self.keys.held(frame.keys, key);
        let edge = |key| held(key) && frame.key_transitions.fresh.contains(&key);
        let axis = |positive, negative| {
            f32::from(u8::from(held(positive))) - f32::from(u8::from(held(negative)))
        };
        result.movement += Vec3::new(
            axis(KeyCode::KeyD, KeyCode::KeyA),
            axis(KeyCode::KeyW, KeyCode::KeyS),
            axis(KeyCode::KeyE, KeyCode::KeyQ),
        );
        result.movement = result
            .movement
            .clamp(Vec3::splat(-1.), Vec3::ONE)
            .clamp_length_max(1.);
        result.look += Vec2::new(
            axis(KeyCode::ArrowRight, KeyCode::ArrowLeft),
            axis(KeyCode::ArrowUp, KeyCode::ArrowDown),
        );
        result.look = result.look.clamp(Vec2::splat(-1.), Vec2::ONE);
        result.fast |= held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight);
        result.reset |= edge(KeyCode::KeyR);
        result.toggle |= edge(KeyCode::Tab);
        result.close |= edge(KeyCode::Escape);
        // Discard transient motion at a boundary. It belongs to the context
        // that had focus when the events arrived, not the newly selected one.
        if !boundary {
            if self.mouse.held(frame.mouse, MouseButton::Right) && frame.motion.is_finite() {
                result.pointer_look = frame.motion.clamp(Vec2::splat(-2000.), Vec2::splat(2000.));
            }
            if frame.scroll.is_finite() {
                result.scroll = frame.scroll.clamp(-20., 20.);
            }
        }
        result
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy system parameters are separate input owners"
)]
fn collect_input(
    context: Res<Context>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    pads: Query<(Entity, &Gamepad)>,
    mut focus: MessageReader<WindowFocused>,
    mut keyboard_focus_lost: MessageReader<KeyboardFocusLost>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut connections: MessageReader<GamepadConnectionEvent>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut boundary: ResMut<Boundary>,
    mut actions: ResMut<Actions>,
) {
    let window = windows.iter().next();
    let focused = window.is_some_and(|(_, window)| window.focused);
    // Winit sends its synthetic Released messages with KeyboardFocusLost on a
    // later frame. Treat that marker as a boundary even after window regain.
    let mut focus_changed = keyboard_focus_lost.read().count() != 0;
    for event in focus.read() {
        focus_changed |= window.is_some_and(|(id, _)| event.window == id);
    }
    let mut key_transitions = KeyTransitions::default();
    for event in keyboard.read() {
        if window.is_some_and(|(id, _)| event.window == id) {
            key_transitions.record(event);
        }
    }
    // Keep the current controller until disconnect. A replacement is selected
    // deterministically, and enters through its own held/neutral boundary.
    let pad = boundary
        .pad
        .and_then(|id| pads.get(id).ok())
        .or_else(|| pads.iter().min_by_key(|(id, _)| id.to_bits()));
    let mut pad_changed = false;
    for event in connections.read() {
        pad_changed |=
            Some(event.gamepad) == boundary.pad || pad.is_some_and(|(id, _)| id == event.gamepad);
    }
    let pointer = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let mut scroll = 0.;
    for event in wheel.read() {
        if window.is_some_and(|(id, _)| id == event.window) {
            // Convert each event before summing; the Bevy accumulator retains
            // only the last unit when pixel and line events share a frame.
            scroll += event.y
                / match event.unit {
                    MouseScrollUnit::Line => 1.,
                    MouseScrollUnit::Pixel => 100.,
                };
        }
    }
    *actions = boundary.sample(Frame {
        context: *context,
        focused,
        focus_changed,
        pad_changed,
        keys: &keys,
        key_transitions,
        mouse: &mouse,
        pad,
        motion: pointer,
        scroll,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{
        InputPlugin,
        gamepad::{GamepadConnection, RawGamepadAxisChangedEvent, RawGamepadEvent},
        keyboard::Key,
    };

    #[derive(Default)]
    struct Devices {
        keys: ButtonInput<KeyCode>,
        mouse: ButtonInput<MouseButton>,
        pad: Gamepad,
    }
    impl Devices {
        fn frame(&self, context: Context, focused: bool, pad_id: Option<Entity>) -> Frame<'_> {
            Frame {
                context,
                focused,
                focus_changed: false,
                pad_changed: false,
                keys: &self.keys,
                // These authored device edges are physical, unlike production
                // ButtonInput edges which may come from focus clearing/repeat.
                key_transitions: KeyTransitions {
                    pressed: self.keys.get_just_pressed().copied().collect(),
                    fresh: self.keys.get_just_pressed().copied().collect(),
                    released: self.keys.get_just_released().copied().collect(),
                },
                mouse: &self.mouse,
                pad: pad_id.map(|id| (id, &self.pad)),
                motion: Vec2::ZERO,
                scroll: 0.,
            }
        }
        fn next(&mut self) {
            self.keys.clear();
            self.mouse.clear();
            self.pad.digital_mut().clear();
        }
    }

    #[test]
    fn keyboard_edges_diagonal_speed_and_context_held_leakage() {
        let mut gate = Boundary::default();
        let mut devices = Devices::default();
        gate.sample(devices.frame(Context::Orbit, true, None));
        devices.keys.press(KeyCode::KeyW);
        devices.keys.press(KeyCode::KeyD);
        devices.keys.press(KeyCode::Tab);
        let action = gate.sample(devices.frame(Context::Orbit, true, None));
        assert!(action.toggle);
        assert!((action.movement.length() - 1.).abs() < 1e-6);
        devices.next();
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None)),
            Actions::default()
        );
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None)),
            Actions::default()
        );
        devices.keys.release_all();
        devices.next();
        gate.sample(devices.frame(Context::Fly, true, None));
        devices.keys.press(KeyCode::KeyW);
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None))
                .movement,
            Vec3::Y
        );
    }

    #[test]
    fn focus_loss_regain_and_suspended_context_require_release() {
        let mut gate = Boundary::default();
        let mut devices = Devices::default();
        gate.sample(devices.frame(Context::Fly, true, None));
        devices.keys.press(KeyCode::KeyW);
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None))
                .movement,
            Vec3::Y
        );
        // Bevy synthesizes this release while the physical key is still down.
        devices.keys.release_all();
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, false, None)),
            Actions::default()
        );
        devices.next();
        devices.keys.press(KeyCode::KeyW);
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None)),
            Actions::default()
        );
        devices.next();
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None)),
            Actions::default()
        );
        devices.keys.release(KeyCode::KeyW);
        devices.next();
        gate.sample(devices.frame(Context::Fly, true, None));
        devices.keys.press(KeyCode::KeyW);
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None))
                .movement,
            Vec3::Y
        );
        assert_eq!(
            gate.sample(devices.frame(Context::Suspended, true, None)),
            Actions::default()
        );
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None)),
            Actions::default()
        );
    }

    #[test]
    fn mouse_drag_and_transient_motion_are_focus_owned() {
        let mut gate = Boundary::default();
        let mut devices = Devices::default();
        gate.sample(devices.frame(Context::Orbit, true, None));
        devices.mouse.press(MouseButton::Right);
        let mut frame = devices.frame(Context::Orbit, true, None);
        frame.motion = Vec2::new(30., -15.);
        frame.scroll = 2.;
        let action = gate.sample(frame);
        assert_eq!(action.pointer_look, Vec2::new(30., -15.));
        assert_eq!(action.scroll, 2.);
        assert_eq!(
            gate.sample(devices.frame(Context::Orbit, false, None)),
            Actions::default()
        );
        let mut frame = devices.frame(Context::Orbit, true, None);
        frame.motion = Vec2::splat(100.);
        frame.scroll = 2.;
        assert_eq!(gate.sample(frame), Actions::default());
        assert_eq!(
            gate.sample(devices.frame(Context::Orbit, true, None))
                .pointer_look,
            Vec2::ZERO
        );
        devices.mouse.release(MouseButton::Right);
        devices.next();
        gate.sample(devices.frame(Context::Orbit, true, None));
        devices.mouse.press(MouseButton::Right);
        let mut frame = devices.frame(Context::Orbit, true, None);
        frame.motion = Vec2::new(4., 3.);
        assert_eq!(gate.sample(frame).pointer_look, Vec2::new(4., 3.));
    }

    #[test]
    fn controller_disconnect_reconnect_and_held_buttons_cannot_leak() {
        let mut gate = Boundary::default();
        let mut devices = Devices::default();
        let mut world = World::new();
        let id = world.spawn_empty().id();
        gate.sample(devices.frame(Context::Fly, true, Some(id)));
        gate.sample(devices.frame(Context::Fly, true, Some(id)));
        devices.pad.analog_mut().set(GamepadAxis::LeftStickY, 1.);
        devices.pad.digital_mut().press(GamepadButton::Select);
        let action = gate.sample(devices.frame(Context::Fly, true, Some(id)));
        assert_eq!(action.movement, Vec3::Y);
        assert!(action.toggle);
        devices.next();
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, None)),
            Actions::default()
        );
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, Some(id))),
            Actions::default()
        );
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, Some(id))),
            Actions::default()
        );
        devices.pad.analog_mut().set(GamepadAxis::LeftStickY, 0.);
        devices.pad.digital_mut().release_all();
        devices.next();
        gate.sample(devices.frame(Context::Fly, true, Some(id)));
        devices.pad.analog_mut().set(GamepadAxis::LeftStickY, 1.);
        assert_eq!(
            gate.sample(devices.frame(Context::Fly, true, Some(id)))
                .movement,
            Vec3::Y
        );
        let mut frame = devices.frame(Context::Fly, true, Some(id));
        frame.pad_changed = true;
        assert_eq!(gate.sample(frame).movement, Vec3::ZERO);
    }

    #[test]
    fn stick_deadzone_keeps_analog_magnitude_and_rejects_bad_data() {
        assert_eq!(stick(Vec2::new(0.1, 0.)), Vec2::ZERO);
        assert!((stick(Vec2::new(0.575, 0.)).x - 0.5).abs() < 1e-6);
        assert!((stick(Vec2::ONE).length() - 1.).abs() < 1e-6);
        assert_eq!(stick(Vec2::new(f32::NAN, 1.)), Vec2::ZERO);
        assert_eq!(stick(Vec2::new(f32::INFINITY, 1.)), Vec2::ZERO);
    }

    #[test]
    fn empty_focus_regain_cannot_arm_repeat_without_physical_release() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            WindowPlugin::default(),
            InspectionInputPlugin,
        ));
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = windows.single(app.world()).unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        let key = |state, repeat| KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state,
            repeat,
            text: None,
            window,
        };
        app.world_mut()
            .write_message(key(ButtonState::Pressed, false));
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.world_mut().write_message(KeyboardFocusLost);
        // Winit's focus clearing also synthesizes this Released message. It is
        // not evidence the user physically released W.
        app.world_mut()
            .write_message(key(ButtonState::Released, false));
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        app.update(); // Winit ignores synthetic regain presses.
        app.update(); // Empty ButtonInput sample must not clear quarantine.
        app.world_mut()
            .write_message(key(ButtonState::Pressed, true));
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        app.world_mut()
            .write_message(key(ButtonState::Released, false));
        app.update();
        app.world_mut()
            .write_message(key(ButtonState::Pressed, false));
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
    }

    fn keyboard_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            WindowPlugin::default(),
            InspectionInputPlugin,
        ));
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = windows.single(app.world()).unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        (app, window)
    }

    fn key(app: &mut App, window: Entity, key_code: KeyCode, state: ButtonState, repeat: bool) {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: match key_code {
                KeyCode::Tab => Key::Tab,
                KeyCode::Escape => Key::Escape,
                _ => Key::Character("w".into()),
            },
            state,
            repeat,
            text: None,
            window,
        });
    }

    fn focus(app: &mut App, window: Entity, focused: bool) {
        app.world_mut().get_mut::<Window>(window).unwrap().focused = focused;
        app.world_mut()
            .write_message(WindowFocused { window, focused });
    }

    #[test]
    fn repeated_shortcuts_stay_quarantined_and_physical_fresh_press_resumes() {
        for code in [KeyCode::Tab, KeyCode::Escape] {
            let (mut app, window) = keyboard_app();
            key(&mut app, window, code, ButtonState::Pressed, false);
            app.update();
            let action = *app.world().resource::<Actions>();
            assert_eq!(action.toggle, code == KeyCode::Tab);
            assert_eq!(action.close, code == KeyCode::Escape);
            focus(&mut app, window, false);
            app.world_mut().write_message(KeyboardFocusLost);
            key(&mut app, window, code, ButtonState::Released, false);
            app.update();
            assert_eq!(*app.world().resource::<Actions>(), Actions::default());
            // The user releases outside this application. That event cannot
            // arm the unfocused context; a later fresh press can establish it.
            key(&mut app, window, code, ButtonState::Released, false);
            app.update();
            focus(&mut app, window, true);
            app.update();
            app.update();
            key(&mut app, window, code, ButtonState::Pressed, true);
            app.update();
            assert_eq!(*app.world().resource::<Actions>(), Actions::default());
            app.update();
            assert_eq!(*app.world().resource::<Actions>(), Actions::default());
            // A non-repeat physical press after the unobserved release arms
            // this shortcut even when ButtonInput still calls the key held.
            key(&mut app, window, code, ButtonState::Pressed, false);
            app.update();
            let action = *app.world().resource::<Actions>();
            assert_eq!(action.toggle, code == KeyCode::Tab);
            assert_eq!(action.close, code == KeyCode::Escape);
        }
    }

    #[test]
    fn delayed_focus_clear_cannot_arm_repeat_after_window_regain() {
        let (mut app, window) = keyboard_app();
        key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed, false);
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
        focus(&mut app, window, false);
        app.update();
        focus(&mut app, window, true);
        app.update();
        // Winit's Last-schedule clear can reach the adapter after focus has
        // returned. Its Released message still cannot prove physical release.
        app.world_mut().write_message(KeyboardFocusLost);
        key(
            &mut app,
            window,
            KeyCode::KeyW,
            ButtonState::Released,
            false,
        );
        app.update();
        app.update();
        key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed, true);
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        key(
            &mut app,
            window,
            KeyCode::KeyW,
            ButtonState::Released,
            false,
        );
        app.update();
        key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed, false);
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
    }

    #[test]
    fn other_window_messages_cannot_remove_primary_keyboard_quarantine() {
        let (mut app, window) = keyboard_app();
        let other = app.world_mut().spawn(Window::default()).id();
        key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed, false);
        app.update();
        focus(&mut app, window, false);
        app.world_mut().write_message(KeyboardFocusLost);
        key(
            &mut app,
            window,
            KeyCode::KeyW,
            ButtonState::Released,
            false,
        );
        app.update();
        focus(&mut app, window, true);
        app.update();
        app.update();
        key(&mut app, other, KeyCode::KeyW, ButtonState::Pressed, false);
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        key(&mut app, other, KeyCode::KeyW, ButtonState::Released, false);
        app.update();
        key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed, true);
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        key(
            &mut app,
            window,
            KeyCode::KeyW,
            ButtonState::Released,
            false,
        );
        app.update();
        key(&mut app, window, KeyCode::KeyW, ButtonState::Pressed, false);
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
    }

    #[test]
    fn bevy_event_pipeline_consumes_focus_mouse_units_and_device_disconnect() {
        // No renderer/window backend: this exercises the actual InputPlugin
        // message processing and our scheduled adapter, rather than mocking it.
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            WindowPlugin::default(),
            InspectionInputPlugin,
        ));
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = query.single(app.world()).unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        let key_event = |state| KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state,
            text: None,
            repeat: false,
            window,
        };
        app.world_mut()
            .write_message(key_event(ButtonState::Pressed));
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.world_mut().write_message(KeyboardFocusLost);
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        app.world_mut()
            .write_message(key_event(ButtonState::Pressed));
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        app.update();
        assert_eq!(*app.world().resource::<Actions>(), Actions::default());
        app.world_mut()
            .write_message(key_event(ButtonState::Released));
        app.update();
        for (unit, y) in [(MouseScrollUnit::Line, 1.), (MouseScrollUnit::Pixel, 100.)] {
            app.world_mut().write_message(MouseWheel {
                unit,
                x: 0.,
                y,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
        }
        app.update();
        assert_eq!(app.world().resource::<Actions>().scroll, 2.);
        let id = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent::new(
            id,
            GamepadConnection::Connected {
                name: "authored controller".into(),
                vendor_id: None,
                product_id: None,
            },
        ));
        app.update();
        app.update();
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                id,
                GamepadAxis::LeftStickY,
                1.,
            )));
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::Y);
        app.world_mut().write_message(GamepadConnectionEvent::new(
            id,
            GamepadConnection::Disconnected,
        ));
        app.update();
        assert_eq!(app.world().resource::<Actions>().movement, Vec3::ZERO);
    }
}
