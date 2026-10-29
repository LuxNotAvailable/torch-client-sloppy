use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused};

use super::systems::{Shared, WorldCamera, enter_screen};
use crate::gui::keybinds::Action;
use crate::session::{
    DropRequest, Gamemode, InvAction, MOVE_BACK, MOVE_DESCEND, MOVE_FORWARD, MOVE_JUMP, MOVE_LEFT,
    MOVE_RIGHT, MOVE_SNEAK, MOVE_SPRINT,
};

#[derive(Resource, Default)]
pub(crate) struct CameraAngles {
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

impl CameraAngles {
    pub(crate) fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw.to_radians())
            * Quat::from_rotation_x(self.pitch.to_radians())
    }
}

#[derive(Resource, Default)]
pub struct FreecamState {
    pub active: bool,
}

#[derive(Resource, Default)]
pub struct ThirdPersonState {
    pub active: bool,
}

pub(super) const THIRD_PERSON_DISTANCE: f32 = 4.0;

pub(crate) fn run_exit_tasks() {
    save_on_session_end();
    crate::client::bot::shutdown_connection();
}

#[cfg(target_arch = "wasm32")]
pub(super) fn run_exit_tasks_on_exit(mut exit: bevy::ecs::message::MessageReader<AppExit>) {
    if exit.read().next().is_some() {
        run_exit_tasks();
    }
}

#[inline]
fn save_on_session_end() {
    crate::modules::save();
    crate::gui::command_history::save();
}

fn reset_session_gui(
    state: &mut crate::gui::GuiState,
    #[cfg(feature = "skins")] faces: &mut crate::gui::player_faces::PlayerFaces,
) {
    save_on_session_end();
    state.chat.reset_for_session();
    state.hud = Default::default();
    state.hud_overlays = Default::default();
    state.toasts = Default::default();
    state.advancements = Default::default();
    #[cfg(feature = "skins")]
    faces.reset();
}

struct NavPoll {
    ended: Option<Option<Vec<crate::text::Span>>>,
    container_id: i32,
    container_kind: crate::session::ContainerKind,
    sign_editor_open: Option<crate::session::SignEditRequest>,
    signing_prompt: bool,
    book_open: Option<crate::session::InteractionHand>,
    book_edit_open: Option<crate::session::BookEditRequest>,
    command_block_open: Option<crate::session::CommandBlockOpen>,
    dialog_show: Option<std::sync::Arc<crate::play::dialog::Dialog>>,
    dialog_clear: bool,
    sleeping: bool,
    dead: bool,
    death_opening: Option<(Option<Vec<crate::text::Span>>, i32, bool)>,
}

pub(super) fn apply_gui_nav(
    shared: Res<Shared>,
    mut state: ResMut<crate::gui::GuiState>,
    mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut exit: bevy::ecs::message::MessageWriter<AppExit>,
    mut last_container_id: Local<i32>,
    #[cfg(feature = "skins")] mut faces: ResMut<crate::gui::player_faces::PlayerFaces>,
) {
    if std::mem::take(&mut state.quit) {
        shared.0.lock().unwrap().quit_requested = true;
        exit.write(AppExit::Success);
        return;
    }
    if std::mem::take(&mut state.disconnect) {
        crate::client::bot::request_disconnect();
        reset_session_gui(
            &mut state,
            #[cfg(feature = "skins")]
            &mut faces,
        );
    }
    use crate::gui::Screen;
    let poll = {
        let mut s = shared.0.lock().unwrap();
        let ended = std::mem::replace(&mut s.disconnected_pending, false)
            .then(|| s.disconnect_reason.take());
        let dead = s.session.dead;
        let death_opening = dead
            && (state.screen == Screen::None
                || state.screen.is_modal()
                || state.screen == Screen::Advancements)
            && s.session.show_death_screen;
        NavPoll {
            ended,
            container_id: s.session.container_id,
            container_kind: s.session.container_kind,
            sign_editor_open: s.session.sign_editor_open.take(),
            signing_prompt: std::mem::take(&mut s.session.chat_signing_prompt),
            book_open: s.session.book_open.take(),
            book_edit_open: s.session.book_edit_open.take(),
            command_block_open: s.session.command_block_open.take(),
            dialog_show: s.session.dialog_show.take(),
            dialog_clear: std::mem::take(&mut s.session.dialog_clear),
            sleeping: s.session.sleeping,
            dead,
            death_opening: death_opening.then(|| {
                (
                    s.session.death_cause.clone(),
                    s.session.death_score,
                    s.session.hardcore,
                )
            }),
        }
    };
    if let Some(reason) = poll.ended {
        reset_session_gui(
            &mut state,
            #[cfg(feature = "skins")]
            &mut faces,
        );
        state.nav = Some(crate::gui::Screen::Disconnected);
        state.disconnect_reason = reason;
    }
    if let Some(address) = state.connect.take() {
        crate::client::bot::start_bot(address);
        let mut s = shared.0.lock().unwrap();
        s.render_distance_sent = None;
        s.skin_prefs_sent = None;
        s.chat_prefs_sent = None;
    }
    let (container_id, kind) = (poll.container_id, poll.container_kind);
    if container_id != *last_container_id && container_id != 0 {
        if kind.is_some() {
            crate::renderer::systems::reset_menu_state(&mut state);
            state.nav = Some(Screen::Container(kind));
        } else if state.screen.is_container() {
            crate::renderer::systems::reset_menu_state(&mut state);
            state.nav = Some(Screen::None);
        }
    } else if container_id == 0 && state.screen.is_container() {
        state.nav = Some(Screen::None);
    }
    *last_container_id = container_id;

    let sign_editor_open = poll.sign_editor_open;
    let signing_prompt = poll.signing_prompt;
    let book_open = poll.book_open;
    let book_edit_open = poll.book_edit_open;
    let command_block_open = poll.command_block_open;
    if let Some(req) = command_block_open {
        state.command_block.open(req.pos, req.mode, req.conditional);
        state.nav = Some(Screen::CommandBlock);
    }
    if let Some(req) = sign_editor_open {
        let pos = req.pos;
        if state.screen == Screen::SignEdit {
            let mut s = shared.0.lock().unwrap();
            s.session.sign_update = Some(state.sign_edit.close(None));
        }
        state.sign_edit.open(req);
        state.nav = Some(Screen::SignEdit);
        shared.0.lock().unwrap().session.sign_edit_open_pos = Some(pos);
    }

    if book_open.is_some() || book_edit_open.is_some() {
        let (view, edit) = {
            let s = shared.0.lock().unwrap();
            let selected = s.session.hotbar_selected;
            let book_of = |hand: crate::session::InteractionHand| {
                s.session
                    .hotbar
                    .get(hand.hotbar_index(selected))
                    .and_then(|slot| slot.book.as_deref())
                    .cloned()
            };
            (
                book_open.and_then(&book_of),
                book_edit_open.and_then(|req| book_of(req.hand).map(|book| (req, book, selected))),
            )
        };
        if let Some(book) = view {
            state.book.open_view(&book);
            state.nav = Some(Screen::BookView);
        } else if let Some((req, book, selected)) = edit {
            state.book.open_edit(&book, req.hand, selected, &req.author);
            state.nav = Some(Screen::BookEdit);
        }
    }

    if let Some(dialog) = poll.dialog_show {
        let current = state.screen;
        state.dialog.show(dialog, current);
        state.nav = Some(Screen::Dialog);
    }
    if poll.dialog_clear && state.screen == Screen::Dialog {
        state.nav = Some(state.dialog.clear());
    }

    if signing_prompt && state.screen != Screen::SignedChatPrompt {
        state.nav = Some(Screen::SignedChatPrompt);
    }

    if state.nav.is_none() {
        let (sleeping, dead) = (poll.sleeping, poll.dead);
        if let Some((cause, score, hardcore)) = poll.death_opening {
            state.death.open(cause, score, hardcore);
            state.nav = Some(Screen::Death);
        } else if !dead && state.screen == Screen::Death {
            state.nav = Some(Screen::None);
        } else if sleeping && matches!(state.screen, Screen::None | Screen::Chat) {
            // In bed the chat stays up, with what was being typed.
            if state.screen == Screen::None {
                state.chat.open("");
            }
            state.nav = Some(Screen::Sleep);
        } else if !sleeping && state.screen == Screen::Sleep {
            // `onPlayerWokeUp`: a message being typed carries on in chat.
            if state.chat.draft_is_empty() {
                state.chat.close();
                state.nav = Some(Screen::None);
            } else {
                state.nav = Some(Screen::Chat);
            }
        }
    }

    let Some(next) = state.nav.take() else { return };
    if next == state.screen {
        return;
    }
    let is_container_screen = |s: Screen| s.is_modal();
    if is_container_screen(state.screen) && !is_container_screen(next) {
        shared
            .0
            .lock()
            .unwrap()
            .session
            .inv_actions
            .push(InvAction::Close);
    }
    if state.screen == Screen::Dialog && next != Screen::Dialog {
        state.dialog.clear();
    }
    if state.screen == Screen::CommandBlock && next != Screen::CommandBlock {
        let mut s = shared.0.lock().unwrap();
        s.session.command_block_pos = None;
        s.session.command_block_data = None;
    }
    if state.screen == Screen::SignEdit && next != Screen::SignEdit {
        let mut s = shared.0.lock().unwrap();
        let rev = s
            .session
            .block_entities
            .get(&state.sign_edit.key())
            .map(|info| info.rev);
        s.session.sign_update = Some(state.sign_edit.close(rev));
        s.session.sign_edit_open_pos = None;
    }
    enter_screen(next, &mut state, &shared, &mut windows);
}

pub(super) fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    #[cfg_attr(feature = "mobile_ui", allow(unused_mut))] mut angles: ResMut<CameraAngles>,
    shared: Res<Shared>,
    freecam: Res<FreecamState>,
    #[cfg_attr(feature = "mobile_ui", allow(unused_mut))] mut camera: Query<
        &mut Transform,
        With<WorldCamera>,
    >,
    state: Res<crate::gui::GuiState>,
    time: Res<Time>,
    mut boat_turn: Local<BoatTurn>,
    zoom: Res<ZoomState>,
) {
    #[cfg(feature = "mobile_ui")]
    {
        let _ = (motion, angles, shared, freecam, camera, state, time, boat_turn, zoom);
        return;
    }
    // The boat being steered turns the view with it. Minecraft's yaw runs
    // the other way to the camera's.
    let nudge = boat_turn.step(
        std::mem::take(&mut shared.0.lock().unwrap().session.vehicle_yaw_nudge),
        time.delta_secs(),
    );
    #[cfg(not(feature = "mobile_ui"))]
    if nudge != 0.0 && !freecam.active {
        angles.yaw -= nudge;
        if let Ok(mut t) = camera.single_mut() {
            t.rotation = angles.rotation();
        }
        let mut s = shared.0.lock().unwrap();
        s.camera_yaw = angles.yaw;
    }
    #[cfg(not(feature = "mobile_ui"))]
    {
        let hand = if state.screen.is_open() || !crate::platform::pointer::is_locked() {
            Vec2::ZERO
        } else {
            // Zoomed in, the mouse slows with the view.
            motion.delta * state.options.look_scale() * zoom.factor
        };
        if hand == Vec2::ZERO && !crate::modules::hooks::drives_look() {
            return;
        }
        apply_look(
            hand.x,
            hand.y,
            &mut angles,
            &mut camera,
            &shared,
            freecam.active,
            time.delta_secs(),
        );
    }
}

/// The steered boat's turn, handed to the view at the pace it arrives.
/// `positionRider` turns only `yRot` once a tick, and the camera draws
/// `lerp(partialTick, yRotO, yRot)`, so vanilla's view glides with the boat.
/// Each tick's turn plays out over a tick's worth of frame time, faster when
/// turns back up, so the view never steps.
#[derive(Default)]
pub(super) struct BoatTurn {
    remaining: f32,
    /// Degrees a second of the last tick's turn.
    rate: f32,
}

const TICK_SECONDS: f32 = 0.05;

impl BoatTurn {
    fn step(&mut self, arrived: f32, dt: f32) -> f32 {
        if arrived != 0.0 {
            self.remaining += arrived;
            self.rate = arrived.abs() / TICK_SECONDS;
        }
        if self.remaining == 0.0 {
            return 0.0;
        }
        // At least the last tick's pace; a backlog drains within a tick.
        let rate = self.rate.max(self.remaining.abs() / TICK_SECONDS);
        let turn = (rate * dt).min(self.remaining.abs()).copysign(self.remaining);
        self.remaining -= turn;
        if self.remaining.abs() < 1e-4 {
            self.remaining = 0.0;
        }
        turn
    }
}

pub(crate) const LOOK_SCALE: f32 = 0.15;

pub(crate) fn apply_look(
    dx: f32,
    dy: f32,
    angles: &mut CameraAngles,
    camera: &mut Query<&mut Transform, With<WorldCamera>>,
    shared: &Shared,
    freecam_active: bool,
    dt: f32,
) {
    let (dx, dy) = if !freecam_active && crate::modules::hooks::shapes_look() {
        let s = shared.0.lock().unwrap();
        crate::modules::hooks::look_delta(&s, angles.yaw, angles.pitch, dt, (dx, dy))
    } else {
        (dx, dy)
    };

    angles.yaw -= dx;
    angles.pitch = (angles.pitch - dy).clamp(-89., 89.);
    // `Entity.turn` then `onPassengerTurned`: a boat's driver looks no more
    // than 105° off its bow. Minecraft's yaw is -camera yaw - 180.
    if !freecam_active
        && let Some(boat_yaw) = shared.0.lock().unwrap().session.steered_boat_yaw
    {
        let yaw = crate::play::boat::clamp_driver_yaw(-angles.yaw - 180.0, boat_yaw);
        angles.yaw = -yaw - 180.0;
    }
    if let Ok(mut t) = camera.single_mut() {
        t.rotation = angles.rotation();
    }
    if freecam_active {
        return;
    }
    let mut s = shared.0.lock().unwrap();
    s.camera_yaw = angles.yaw;
    s.camera_pitch = angles.pitch;
}

pub(crate) fn bot_movement_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    freecam: Res<FreecamState>,
    time: Res<Time>,
    shared: Res<Shared>,
    state: Res<crate::gui::GuiState>,
    mut intent: ResMut<crate::renderer::intent::Intent>,
    mut last_forward_tap: Local<Option<f32>>,
    mut sprint_latch: Local<bool>,
    mut toggled: Local<Toggled>,
) {
    if freecam.active || state.screen.is_open() || crate::gui::keyboard_elsewhere() {
        *last_forward_tap = None;
        *sprint_latch = false;
        return;
    }
    let options = &state.options;
    toggled.update(
        options.toggle_crouch,
        state.keybinds.just(Action::Sneak, &keys, &buttons),
        options.toggle_sprint,
        state.keybinds.just(Action::Sprint, &keys, &buttons),
    );
    let binds = &state.keybinds;
    let down = |action| binds.down(action, &keys, &buttons);
    let fwd = down(Action::Forward);
    let back = down(Action::Back);
    let left = down(Action::Left);
    let right = down(Action::Right);
    let jump = down(Action::Jump);
    let shift = if options.toggle_crouch {
        toggled.crouch
    } else {
        down(Action::Sneak)
    };
    let sprint_key = if options.toggle_sprint {
        toggled.sprint
    } else {
        down(Action::Sprint)
    };

    let flying = {
        let mut s = shared.0.lock().unwrap();
        s.session.auto_jump = state.options.auto_jump;
        s.session.flying
    };
    let now = time.elapsed_secs();
    if binds.just(Action::Forward, &keys, &buttons) {
        if last_forward_tap.is_some_and(|t| now - t <= FLY_DOUBLE_TAP_SECS) {
            *sprint_latch = true;
            *last_forward_tap = None;
        } else {
            *last_forward_tap = Some(now);
        }
    }
    if !fwd || shift {
        *sprint_latch = false;
    }

    let sprint = sprint_key || *sprint_latch;
    let descend = flying && shift;

    intent.walk(
        (fwd as u8) * MOVE_FORWARD
            | (back as u8) * MOVE_BACK
            | (left as u8) * MOVE_LEFT
            | (right as u8) * MOVE_RIGHT
            | (jump as u8) * MOVE_JUMP
            | (sprint as u8) * MOVE_SPRINT
            | (descend as u8) * MOVE_DESCEND
            | (shift as u8) * MOVE_SNEAK,
    );
}

/// Latched sneak and sprint for the toggle options; a latch drops when its
/// option is switched back to hold.
#[derive(Default)]
pub(super) struct Toggled {
    crouch: bool,
    sprint: bool,
}

impl Toggled {
    fn update(&mut self, crouch_on: bool, crouch_tap: bool, sprint_on: bool, sprint_tap: bool) {
        self.crouch = crouch_on && (self.crouch != crouch_tap);
        self.sprint = sprint_on && (self.sprint != sprint_tap);
    }
}

pub(super) const FLY_DOUBLE_TAP_SECS: f32 = 7.0 / 20.0;

pub(super) fn flight_toggle_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    freecam: Res<FreecamState>,
    time: Res<Time>,
    shared: Res<Shared>,
    mut last_tap: Local<Option<f32>>,
    state: Res<crate::gui::GuiState>,
) {
    if !state.keybinds.just(Action::Jump, &keys, &buttons)
        || freecam.active
        || state.screen.is_open()
    {
        return;
    }
    let now = time.elapsed_secs();
    let doubled = last_tap.is_some_and(|t| now - t <= FLY_DOUBLE_TAP_SECS);
    if !doubled {
        *last_tap = Some(now);
        return;
    }
    *last_tap = None;

    let mut s = shared.0.lock().unwrap();
    if matches!(s.session.gamemode, Gamemode::Creative | Gamemode::Spectator) {
        s.session.fly_toggle = true;
    }
}

/// Lets the cursor go when the window loses focus, and takes it back when
/// the window regains focus with no screen open, so the game has the mouse
/// again without a trip through the pause menu. Coming back with the cursor
/// outside the window, the OS can refuse the grab for a frame or two while
/// the window comes to the front, so the grab is pressed again for a few
/// frames, each time with the cursor put back in the window's centre.
pub(super) fn window_focus_cursor(
    mut events: bevy::ecs::message::MessageReader<WindowFocused>,
    mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    state: Res<crate::gui::GuiState>,
    mut regrab: Local<u8>,
) {
    const REGRAB_FRAMES: u8 = 10;
    for ev in events.read() {
        let Ok((_, opts)) = windows.single_mut() else {
            continue;
        };
        if !ev.focused {
            *regrab = 0;
            super::systems::apply_cursor(opts, CursorGrabMode::None, true);
        } else if !state.screen.is_open() {
            *regrab = REGRAB_FRAMES;
        }
    }
    if *regrab == 0 {
        return;
    }
    *regrab -= 1;
    let Ok((window, opts)) = windows.single_mut() else {
        return;
    };
    if !window.focused || state.screen.is_open() {
        *regrab = 0;
        return;
    }
    let (grab, visible) = super::systems::world_cursor();
    if grab != CursorGrabMode::None {
        crate::platform::pointer::lock();
    }
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    {
        let mut window = window;
        let center = Vec2::new(window.width() / 2.0, window.height() / 2.0);
        window.set_cursor_position(Some(center));
    }
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    let _ = window;
    // Written every time, so the window backend sends the grab again.
    super::systems::apply_cursor(opts, grab, visible);
}

pub(super) fn track_lifecycle(
    mut lifecycle: bevy::ecs::message::MessageReader<bevy::window::AppLifecycle>,
) {
    for event in lifecycle.read() {
        match event {
            bevy::window::AppLifecycle::Suspended => crate::platform::notify::suspended(),
            bevy::window::AppLifecycle::Running => crate::platform::notify::resumed(),
            _ => {}
        }
    }
}

pub(super) fn web_pointer_lock(
    buttons: Res<ButtonInput<MouseButton>>,
    shared: Res<Shared>,
    #[cfg_attr(feature = "mobile_ui", allow(unused_mut))] mut state: ResMut<crate::gui::GuiState>,
    #[cfg_attr(feature = "mobile_ui", allow(unused_mut))] mut windows: Query<
        (&mut Window, &mut CursorOptions),
        With<PrimaryWindow>,
    >,
    mut held_frames: Local<u32>,
) {
    #[cfg(feature = "mobile_ui")]
    {
        let _ = (buttons, shared, state, windows, held_frames);
        return;
    }
    #[cfg(not(feature = "mobile_ui"))]
    {
        if !cfg!(target_arch = "wasm32") {
            return;
        }
        let claimed = crate::platform::pointer::take_claimed();
        if state.screen.is_open() {
            *held_frames = 0;
            return;
        }
        let pressed =
            buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right);
        if crate::platform::pointer::is_locked() {
            if claimed && pressed {
                state.suppress_click = true;
            }
            *held_frames = held_frames.saturating_add(1);
            return;
        }
        const SETTLED: u32 = 3;
        if std::mem::take(&mut *held_frames) >= SETTLED {
            enter_screen(crate::gui::Screen::Pause, &mut state, &shared, &mut windows);
            return;
        }
        if pressed {
            crate::platform::pointer::lock();
            state.suppress_click = true;
        }
    }
}

pub(super) fn web_fullscreen(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    state: Res<crate::gui::GuiState>,
    mut captured: Local<bool>,
) {
    if state.keybinds.just(Action::Fullscreen, &keys, &buttons) {
        crate::platform::fullscreen::toggle();
    }
    let fullscreen = crate::platform::fullscreen::is_fullscreen();
    if fullscreen != *captured {
        *captured = fullscreen;
        crate::platform::fullscreen::capture_escape(fullscreen);
    }
}

pub(super) fn poll_module_binds(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    state: Res<crate::gui::GuiState>,
) {
    use crate::gui::keybinds::Bound;
    use crate::modules::store;

    if state.screen.is_open() {
        return;
    }
    for id in crate::modules::registry::Id::ALL {
        let pressed = match store().bind(id) {
            Bound::Unbound => false,
            Bound::Key(code) => keys.just_pressed(code),
            Bound::Mouse(button) => buttons.just_pressed(button),
        };
        if pressed {
            store().toggle(id);
        }
    }
}

pub(super) fn toggle_third_person(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut third_person: ResMut<ThirdPersonState>,
    state: Res<crate::gui::GuiState>,
) {
    if state.screen.is_open() {
        return;
    }
    if state
        .keybinds
        .just(Action::TogglePerspective, &keys, &buttons)
    {
        third_person.active = !third_person.active;
    }
}

pub(super) fn toggle_freecam(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut freecam: ResMut<FreecamState>,
    mut angles: ResMut<CameraAngles>,
    shared: Res<Shared>,
    mut camera: Query<&mut Transform, With<WorldCamera>>,
    state: Res<crate::gui::GuiState>,
) {
    if state.screen.is_open() {
        return;
    }
    if state.keybinds.just(Action::Freecam, &keys, &buttons) {
        crate::modules::freecam::toggle();
    }
    let want = crate::modules::freecam::active();

    if want != freecam.active {
        freecam.active = want;
        if !freecam.active {
            let s = shared.0.lock().unwrap();
            angles.yaw = s.camera_yaw;
            angles.pitch = s.camera_pitch;
            drop(s);
            if let Ok(mut t) = camera.single_mut() {
                t.rotation = angles.rotation();
            }
        }
    }
}

pub(super) fn freecam_movement(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    freecam: Res<FreecamState>,
    angles: Res<CameraAngles>,
    mut camera: Query<&mut Transform, With<WorldCamera>>,
    time: Res<Time>,
    state: Res<crate::gui::GuiState>,
) {
    if !freecam.active || state.screen.is_open() {
        return;
    }
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };

    let binds = &state.keybinds;
    let down = |action| binds.down(action, &keys, &buttons);
    let speed = crate::modules::freecam::speed(down(Action::Sprint));
    let dt = time.delta_secs();

    let yaw_rad = angles.yaw.to_radians();
    let forward = Vec3::new(-yaw_rad.sin(), 0.0, -yaw_rad.cos());
    let right = Vec3::new(yaw_rad.cos(), 0.0, -yaw_rad.sin());

    if down(Action::Forward) {
        transform.translation += forward * speed * dt;
    }
    if down(Action::Back) {
        transform.translation -= forward * speed * dt;
    }
    if down(Action::Right) {
        transform.translation += right * speed * dt;
    }
    if down(Action::Left) {
        transform.translation -= right * speed * dt;
    }
    if down(Action::Jump) {
        transform.translation.y += speed * dt;
    }
    if down(Action::Sneak) {
        transform.translation.y -= speed * dt;
    }
}

pub(crate) fn handle_mouse_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    shared: Res<Shared>,
    #[cfg_attr(feature = "mobile_ui", allow(unused_mut))] mut state: ResMut<crate::gui::GuiState>,
    #[cfg_attr(feature = "mobile_ui", allow(unused_mut))] mut intent: ResMut<
        crate::renderer::intent::Intent,
    >,
) {
    #[cfg(feature = "mobile_ui")]
    {
        let _ = (buttons, keys, shared, state, intent);
        return;
    }
    #[cfg(not(feature = "mobile_ui"))]
    handle_mouse_input_desktop(buttons, keys, shared, state, intent);
}

#[cfg(not(feature = "mobile_ui"))]
fn handle_mouse_input_desktop(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    shared: Res<Shared>,
    mut state: ResMut<crate::gui::GuiState>,
    mut intent: ResMut<crate::renderer::intent::Intent>,
) {
    if state.screen.is_open() {
        shared.0.lock().unwrap().session.clear_clicks();
        return;
    }
    if std::mem::take(&mut state.suppress_click) {
        shared.0.lock().unwrap().session.clear_clicks();
        return;
    }
    let binds = &state.keybinds;
    intent.hold(
        binds.down(Action::Attack, &keys, &buttons),
        binds.down(Action::Use, &keys, &buttons),
    );
    intent.click(
        binds.just(Action::Attack, &keys, &buttons),
        binds.just(Action::Use, &keys, &buttons),
    );
    if binds.just(Action::PickItem, &keys, &buttons) {
        intent.pick(keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight));
    }
}

pub(super) fn handle_hotbar_keys(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    shared: Res<Shared>,
    mut state: ResMut<crate::gui::GuiState>,
) {
    if state.screen.is_open() {
        return;
    }
    for (i, action) in crate::gui::keybinds::HOTBAR.into_iter().enumerate() {
        if state.keybinds.just(action, &keys, &buttons) {
            let mut s = shared.0.lock().unwrap();
            if s.session.gamemode == Gamemode::Spectator {
                let tab_list = s.session.tab_list.clone();
                drop(s);
                if let Some(uuid) =
                    state
                        .spectator_menu
                        .on_hotbar_selected(i as u8, &tab_list, time.elapsed_secs())
                {
                    shared.0.lock().unwrap().session.spectator_teleport_target = Some(uuid);
                }
            } else {
                s.session.hotbar_target = Some(i as u8);
            }
            break;
        }
    }
}

pub(super) fn handle_spectator_action_key(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    shared: Res<Shared>,
    mut state: ResMut<crate::gui::GuiState>,
) {
    if state.screen.is_open() {
        return;
    }
    if !state
        .keybinds
        .just(Action::SpectatorAction, &keys, &buttons)
    {
        return;
    }
    let s = shared.0.lock().unwrap();
    if s.session.gamemode != Gamemode::Spectator {
        return;
    }
    let tab_list = s.session.tab_list.clone();
    drop(s);
    if let Some(uuid) = state
        .spectator_menu
        .on_action_key(&tab_list, time.elapsed_secs())
    {
        shared.0.lock().unwrap().session.spectator_teleport_target = Some(uuid);
    }
}

pub(crate) fn wheel_notches(
    wheel: &AccumulatedMouseScroll,
    options: &crate::gui::GuiOptions,
) -> Vec2 {
    const PIXELS_PER_NOTCH: f32 = 100.0;
    #[cfg(target_arch = "wasm32")]
    const LINES_PER_NOTCH: f32 = 3.0;
    #[cfg(not(target_arch = "wasm32"))]
    const LINES_PER_NOTCH: f32 = 1.0;

    let per = match wheel.unit {
        MouseScrollUnit::Line => LINES_PER_NOTCH,
        MouseScrollUnit::Pixel => PIXELS_PER_NOTCH,
    };
    options.scroll(wheel.delta / per)
}

pub(crate) fn accumulate_scroll(acc: &mut (f64, f64), dx: f64, dy: f64) -> (i32, i32) {
    if acc.0 != 0.0 && dx.signum() != acc.0.signum() {
        acc.0 = 0.0;
    }
    if acc.1 != 0.0 && dy.signum() != acc.1.signum() {
        acc.1 = 0.0;
    }
    acc.0 += dx;
    acc.1 += dy;
    let (wx, wy) = (acc.0.trunc() as i32, acc.1.trunc() as i32);
    if wx == 0 && wy == 0 {
        return (0, 0);
    }
    acc.0 -= wx as f64;
    acc.1 -= wy as f64;
    (wx, wy)
}

pub(super) fn next_scroll_selection(wheel: f64, current: i32, limit: i32) -> i32 {
    let step = if wheel > 0.0 {
        1
    } else if wheel < 0.0 {
        -1
    } else {
        0
    };
    let mut selected = (current - step).max(-1);
    while selected < 0 {
        selected += limit;
    }
    while selected >= limit {
        selected -= limit;
    }
    selected
}

/// The zoom key: held, the view narrows to a share of the FOV, and the
/// wheel narrows or widens it further instead of turning the hotbar.
#[derive(Resource)]
pub(crate) struct ZoomState {
    pub held: bool,
    /// How many times narrower than the FOV the zoomed view is.
    pub divisor: f32,
    /// The view's FOV this frame over the unzoomed one, which slows the
    /// mouse to match.
    pub factor: f32,
    /// The furthest the wheel zooms in.
    pub max: f32,
}

impl Default for ZoomState {
    fn default() -> Self {
        ZoomState {
            held: false,
            divisor: ZOOM_DEFAULT,
            factor: 1.0,
            max: ZOOM_MAX,
        }
    }
}

const ZOOM_DEFAULT: f32 = 4.0;
const ZOOM_MIN: f32 = 1.5;
const ZOOM_MAX: f32 = 50.0;
/// Each wheel notch zooms this much further.
const ZOOM_STEP: f32 = 1.2;

impl ZoomState {
    /// One wheel movement while zoomed: up zooms in, down zooms out.
    pub(crate) fn scroll(&mut self, notches: f32) {
        self.divisor = (self.divisor * ZOOM_STEP.powf(notches)).clamp(ZOOM_MIN, self.max.max(ZOOM_MIN));
    }
}

pub(super) fn handle_zoom(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    wheel: Res<AccumulatedMouseScroll>,
    mut state: ResMut<crate::gui::GuiState>,
    mut zoom: ResMut<ZoomState>,
) {
    let held = !state.screen.is_open()
        && state.keybinds.capturing.is_none()
        && state.keybinds.down(Action::Zoom, &keys, &buttons);
    let settings = state.options.zoom;
    zoom.max = settings.max_zoom();
    if held && !zoom.held {
        // Each press starts from the default zoom.
        zoom.divisor = settings.default_zoom();
    }
    zoom.held = held;
    if state.zooming != held {
        state.zooming = held;
    }
    if held {
        let notches = wheel_notches(&wheel, &state.options).y;
        if notches != 0.0 {
            zoom.scroll(notches);
        }
    }
}

pub(super) fn handle_hotbar_scroll(
    wheel: Res<AccumulatedMouseScroll>,
    shared: Res<Shared>,
    state: Res<crate::gui::GuiState>,
    zoom: Res<ZoomState>,
    mut acc: Local<(f64, f64)>,
) {
    if state.screen.is_open() || zoom.held {
        return;
    }
    let notches = wheel_notches(&wheel, &state.options);
    let (dx, dy) = (notches.x as f64, notches.y as f64);
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let mut s = shared.0.lock().unwrap();
    if s.session.gamemode == Gamemode::Spectator {
        return;
    }
    let (wx, wy) = accumulate_scroll(&mut acc, dx, dy);
    if wx == 0 && wy == 0 {
        return;
    }
    let notches = if wy == 0 { -wx } else { wy };

    let current = s.session.hotbar_target.unwrap_or(s.session.hotbar_selected) as i32;
    let next = next_scroll_selection(notches as f64, current, HOTBAR_SLOTS);
    if next != current {
        s.session.hotbar_target = Some(next as u8);
    }
}

pub(super) const HOTBAR_SLOTS: i32 = 9;

pub(super) fn handle_spectator_fly_speed_scroll(
    wheel: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    shared: Res<Shared>,
    mut state: ResMut<crate::gui::GuiState>,
    zoom: Res<ZoomState>,
    mut acc: Local<(f64, f64)>,
) {
    if state.screen.is_open() || zoom.held {
        return;
    }
    let dy = wheel_notches(&wheel, &state.options).y;
    if dy == 0.0 {
        return;
    }
    let mut s = shared.0.lock().unwrap();
    if s.session.gamemode != Gamemode::Spectator {
        return;
    }
    if !state.spectator_menu.is_open() {
        s.session.fly_speed_scroll += dy * 0.005;
        return;
    }
    let tab_list = s.session.tab_list.clone();
    drop(s);
    let (_, notches) = accumulate_scroll(&mut acc, 0.0, dy as f64);
    if notches != 0 {
        state
            .spectator_menu
            .on_scroll(notches, &tab_list, time.elapsed_secs());
    }
}

pub(super) fn toggle_screen(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    shared: Res<Shared>,
    mut state: ResMut<crate::gui::GuiState>,
    mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    use crate::gui::Screen;

    if crate::gui::keyboard_elsewhere() {
        return;
    }

    let field_escape = !state.screen.is_menu() && crate::platform::keyboard::take_escape();

    let guard = std::mem::take(&mut state.keybinds.escape_guard);
    if state.keybinds.capturing.is_some() || (guard && keys.just_pressed(KeyCode::Escape)) {
        return;
    }

    #[cfg(feature = "click_gui")]
    {
        let click_gui = state.keybinds.just(Action::ClickGui, &keys, &buttons);
        if state.screen == Screen::ClickGui {
            let escape =
                (keys.just_pressed(KeyCode::Escape) || field_escape) && !state.clickgui.escape();
            if click_gui || escape {
                let back = state.clickgui_parent;
                enter_screen(back, &mut state, &shared, &mut windows);
            }
            return;
        }
        if click_gui && (state.screen == Screen::None || state.screen.is_menu()) {
            state.clickgui_parent = state.screen;
            enter_screen(Screen::ClickGui, &mut state, &shared, &mut windows);
            return;
        }
    }

    #[cfg(feature = "hud_editor")]
    {
        let hud_key = state.keybinds.just(Action::HudEditor, &keys, &buttons);
        if state.screen == Screen::HudEditor {
            let escape =
                (keys.just_pressed(KeyCode::Escape) || field_escape) && !state.hud_state.escape();
            if hud_key || escape {
                let back = state.hud_state.parent;
                enter_screen(back, &mut state, &shared, &mut windows);
            }
            return;
        }
        if hud_key && (state.screen == Screen::None || state.screen.is_menu()) {
            state.hud_state.parent = state.screen;
            state.hud_state.over_menu = state.screen.is_menu();
            enter_screen(Screen::HudEditor, &mut state, &shared, &mut windows);
            return;
        }
    }

    let creative_search_focused =
        state.screen == Screen::Creative && state.creative.search_focused();
    let toggle_key = state.keybinds.just(Action::Inventory, &keys, &buttons)
        && !state.screen.chat_open()
        && state.screen != Screen::SignEdit
        && state.screen != Screen::CommandBlock
        && !matches!(
            state.screen,
            Screen::BookView | Screen::BookEdit | Screen::BookSign
        )
        && !state.screen.is_local_overlay()
        && !creative_search_focused
        && !state.anvil_typing();
    let close_key = keys.just_pressed(KeyCode::Escape) || field_escape;
    #[cfg(target_arch = "wasm32")]
    let pause_key = state.keybinds.just(Action::Pause, &keys, &buttons);
    #[cfg(not(target_arch = "wasm32"))]
    let pause_key = false;
    let enter_from_field = state.screen.chat_open() && crate::platform::keyboard::take_enter();
    let submit_key = state.screen.chat_open()
        && (keys.just_pressed(KeyCode::Enter)
            || keys.just_pressed(KeyCode::NumpadEnter)
            || enter_from_field);

    if state.screen.is_menu() {
        return;
    }

    let next = if state.screen == Screen::Pause || state.screen.is_options() {
        if close_key || (pause_key && state.screen == Screen::Pause) {
            #[cfg(feature = "shader_support")]
            let stepped_within =
                state.screen == Screen::ShaderOptions && state.shaderpacks.pop_screen();
            #[cfg(not(feature = "shader_support"))]
            let stepped_within = false;

            if stepped_within {
                None
            } else {
                Some(match state.screen {
                    Screen::Options => state.options_parent,
                    Screen::MouseSettings => Screen::Controls,
                    Screen::ZoomSettings
                    | Screen::BlockInfoSettings
                    | Screen::InventorySortSettings => Screen::Features,
                    #[cfg(feature = "shader_support")]
                    Screen::ShaderPacks => Screen::VideoSettings,
                    #[cfg(feature = "shader_support")]
                    Screen::ShaderOptions => Screen::ShaderPacks,
                    #[cfg(explore_modrinth)]
                    Screen::ExplorePacks => Screen::ResourcePacks,
                    #[cfg(voice)]
                    Screen::VoiceChat => Screen::Options,
                    #[cfg(voice)]
                    Screen::VoicePlayers | Screen::VoiceGroups => Screen::VoiceChat,
                    s if s.is_options() => Screen::Options,
                    _ => Screen::None,
                })
            }
        } else {
            None
        }
    } else if state.screen == Screen::Chat {
        if (close_key || submit_key) && state.chat.dismiss_link_prompt() {
            None
        } else if close_key && state.chat.hide_suggestions() {
            None
        } else if close_key || submit_key {
            if submit_key && let Some(line) = state.chat.submit() {
                let mut locked = shared.0.lock().unwrap();
                locked.session.outgoing_chat.push(line);
            }
            if submit_key && crate::gui::chat::texting() {
                None
            } else {
                state.chat.close();
                Some(Screen::None)
            }
        } else {
            None
        }
    } else if state.screen == Screen::CommandBlock {
        if close_key && state.command_block.hide_suggestions() {
            None
        } else if close_key {
            Some(Screen::None)
        } else {
            None
        }
    } else if state.screen == Screen::Dialog {
        if close_key {
            let out = state.dialog.on_escape(&shared.0);
            if let Some(command) = out.chat {
                state.chat.open(&command);
                Some(Screen::Chat)
            } else {
                out.nav
            }
        } else {
            None
        }
    } else if state.screen == Screen::Sleep {
        // `InBedChatScreen`: Enter sends and stays in bed; Escape, once the
        // link prompt and suggestions are out of the way, gets up.
        if (close_key || submit_key) && state.chat.dismiss_link_prompt() {
        } else if close_key && state.chat.hide_suggestions() {
        } else if submit_key {
            if let Some(line) = state.chat.submit() {
                shared.0.lock().unwrap().session.outgoing_chat.push(line);
            }
        } else if close_key {
            shared.0.lock().unwrap().leave_bed_requested = true;
        }
        None
    } else if state.screen == Screen::Death {
        None
    } else if state.screen == Screen::Advancements {
        if close_key {
            Some(state.advancements.parent)
        } else if state.keybinds.just(Action::Advancements, &keys, &buttons) {
            Some(Screen::None)
        } else {
            None
        }
    } else if state.screen.is_open() {
        if toggle_key || close_key {
            Some(Screen::None)
        } else {
            None
        }
    } else if toggle_key {
        let creative = shared.0.lock().unwrap().session.gamemode == Gamemode::Creative;
        if creative {
            state.creative.open();
        }
        Some(if creative {
            Screen::Creative
        } else {
            Screen::Inventory
        })
    } else if state.keybinds.just(Action::Advancements, &keys, &buttons) {
        state.advancements.parent = Screen::None;
        Some(Screen::Advancements)
    } else if state.keybinds.just(Action::Chat, &keys, &buttons) {
        state.chat.open("");
        Some(Screen::Chat)
    } else if state.keybinds.just(Action::Command, &keys, &buttons) {
        state.chat.open("/");
        Some(Screen::Chat)
    } else if close_key || pause_key {
        Some(Screen::Pause)
    } else {
        None
    };

    let Some(next) = next else { return };
    if next == Screen::None
        && !matches!(state.screen, Screen::Chat | Screen::Pause)
        && !state.screen.is_options()
        && !state.screen.is_local_overlay()
    {
        shared
            .0
            .lock()
            .unwrap()
            .session
            .inv_actions
            .push(InvAction::Close);
    }
    enter_screen(next, &mut state, &shared, &mut windows);
}

fn debug_combo(
    action: Action,
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    state: &crate::gui::GuiState,
) -> bool {
    !state.screen.is_open()
        && state.keybinds.down(Action::DebugModifier, keys, buttons)
        && state.keybinds.just(action, keys, buttons)
}

pub(super) fn toggle_advanced_tooltips(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<crate::gui::GuiState>,
    shared: Res<Shared>,
) {
    if debug_combo(Action::AdvancedTooltips, &keys, &buttons, &state) {
        state.advanced_tooltips = !state.advanced_tooltips;
        shared.0.lock().unwrap().session.advanced_tooltips = state.advanced_tooltips;
    }
}

pub(super) fn toggle_chunk_borders(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<crate::gui::GuiState>,
) {
    if debug_combo(Action::ChunkBorders, &keys, &buttons, &state) {
        state.chunk_borders = !state.chunk_borders;
    }
}

pub(super) fn reload_all_chunks(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    state: Res<crate::gui::GuiState>,
    shared: Res<Shared>,
) {
    if debug_combo(Action::ReloadChunks, &keys, &buttons, &state) {
        shared.0.lock().unwrap().reload_chunks_requested = true;
    }
}

pub(super) fn handle_keybinds(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<crate::gui::GuiState>,
    shared: Res<Shared>,
    mut commands: Commands,
) {
    let held = state.keybinds.down(Action::PlayerList, &keys, &buttons) && !state.screen.is_open();
    if state.tab_list_held != held {
        state.tab_list_held = held;
    }

    #[cfg(voice)]
    {
        // Push-to-talk and whisper are held; a screen open lets them go,
        // as the mod's key handling does.
        let in_game = !state.screen.is_open();
        crate::voice::set_push_to_talk(
            in_game && state.keybinds.down(Action::VoicePushToTalk, &keys, &buttons),
        );
        crate::voice::set_whisper(
            in_game && state.keybinds.down(Action::VoiceWhisper, &keys, &buttons),
        );
        if in_game && state.keybinds.just(Action::VoiceMute, &keys, &buttons) {
            state.options.voice.muted = !state.options.voice.muted;
            state.options.voice.apply();
            crate::gui::save_options(&state.options);
        }
        if in_game && state.keybinds.just(Action::VoiceDisable, &keys, &buttons) {
            state.options.voice.disabled = !state.options.voice.disabled;
            state.options.voice.apply();
            crate::gui::save_options(&state.options);
        }
    }

    if state.screen.is_open() {
        return;
    }

    if state.keybinds.just(Action::ToggleGui, &keys, &buttons) {
        state.hide_gui = !state.hide_gui;
    }

    if state.keybinds.just(Action::Screenshot, &keys, &buttons) {
        super::screenshot::request(&mut commands);
    }

    if state.keybinds.just(Action::SwapOffhand, &keys, &buttons) {
        shared.0.lock().unwrap().session.swap_hands = true;
    }

    if state.keybinds.just(Action::Drop, &keys, &buttons) {
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        shared.0.lock().unwrap().session.pending_drop = Some(if ctrl {
            DropRequest::HeldStack
        } else {
            DropRequest::HeldItem
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{BoatTurn, ZoomState};

    #[test]
    fn the_wheel_zooms_within_limits() {
        let mut zoom = ZoomState::default();
        zoom.scroll(1.0);
        assert!((zoom.divisor - 4.8).abs() < 1e-4);
        zoom.scroll(-2.0);
        assert!(zoom.divisor < 4.0);
        zoom.scroll(100.0);
        assert_eq!(zoom.divisor, 50.0);
        zoom.scroll(-100.0);
        assert_eq!(zoom.divisor, 1.5);
    }

    #[test]
    fn a_boat_turn_plays_out_over_a_tick() {
        let mut t = BoatTurn::default();
        assert!((t.step(4.0, 0.0125) - 1.0).abs() < 1e-4);
        assert!((t.step(0.0, 0.025) - 2.0).abs() < 1e-4);
        assert!((t.step(0.0, 0.05) - 1.0).abs() < 1e-4);
        assert_eq!(t.step(0.0, 0.05), 0.0);
        // Turns arriving faster than frames drain: never more than a tick behind.
        t.step(4.0, 0.0);
        t.step(4.0, 0.0);
        assert!((t.step(0.0, 0.05) - 8.0).abs() < 1e-4);
    }


    use super::{HOTBAR_SLOTS, accumulate_scroll, next_scroll_selection};

    #[test]
    fn a_toggle_latches_on_tap_and_drops_with_its_option() {
        let mut t = super::Toggled::default();
        t.update(true, true, false, true);
        assert!(t.crouch && !t.sprint);
        t.update(true, false, false, false);
        assert!(t.crouch);
        t.update(true, true, false, false);
        assert!(!t.crouch);
        t.update(true, true, false, false);
        t.update(false, false, false, false);
        assert!(!t.crouch);
    }

    #[test]
    fn default_sensitivity_is_vanillas_look_scale() {
        let mut o = crate::gui::GuiOptions::default();
        o.mouse_sensitivity = crate::gui::MOUSE_SENSITIVITY_DEFAULT;
        o.invert_x = false;
        o.invert_y = true;
        let k = o.look_scale();
        assert!((k.x - super::LOOK_SCALE).abs() < 1e-6);
        assert!((k.y + super::LOOK_SCALE).abs() < 1e-6);
        o.discrete_scrolling = true;
        assert_eq!(
            o.scroll(bevy::math::Vec2::new(0.0, -2.5)),
            bevy::math::Vec2::new(0.0, -1.0)
        );
    }

    #[test]
    fn scroll_direction_matches_vanilla() {
        assert_eq!(next_scroll_selection(1.0, 4, HOTBAR_SLOTS), 3);
        assert_eq!(next_scroll_selection(-1.0, 4, HOTBAR_SLOTS), 5);
        assert_eq!(next_scroll_selection(0.0, 4, HOTBAR_SLOTS), 4);
    }

    #[test]
    fn scroll_wraps_at_both_ends() {
        assert_eq!(next_scroll_selection(1.0, 0, HOTBAR_SLOTS), 8);
        assert_eq!(next_scroll_selection(-1.0, 8, HOTBAR_SLOTS), 0);
    }

    #[test]
    fn multi_notch_scroll_moves_one_slot() {
        assert_eq!(next_scroll_selection(3.0, 4, HOTBAR_SLOTS), 3);
        assert_eq!(next_scroll_selection(-7.0, 4, HOTBAR_SLOTS), 5);
        assert_eq!(next_scroll_selection(9.0, 0, HOTBAR_SLOTS), 8);
    }

    #[test]
    fn scroll_accumulator_banks_fractions() {
        let mut acc = (0.0, 0.0);
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 0.4), (0, 0));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 0.4), (0, 0));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 0.4), (0, 1));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 0.9), (0, 1));
    }

    #[test]
    fn scroll_accumulator_resets_on_reversal() {
        let mut acc = (0.0, 0.0);
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 0.9), (0, 0));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, -0.9), (0, 0));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, -0.2), (0, -1));
    }

    #[test]
    fn scroll_accumulator_passes_whole_notches() {
        let mut acc = (0.0, 0.0);
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 1.0), (0, 1));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, -1.0), (0, -1));
        assert_eq!(accumulate_scroll(&mut acc, 0.0, 2.0), (0, 2));
        assert_eq!(acc.1, 0.0);
    }

    #[test]
    fn repeated_notches_walk_the_hotbar() {
        let mut slot = 1;
        for _ in 0..3 {
            slot = next_scroll_selection(1.0, slot, HOTBAR_SLOTS);
        }
        assert_eq!(slot, 7);
        for _ in 0..3 {
            slot = next_scroll_selection(-1.0, slot, HOTBAR_SLOTS);
        }
        assert_eq!(slot, 1);
    }
}

