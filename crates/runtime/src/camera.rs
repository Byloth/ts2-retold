//! A camera turned by the mouse around the character (ADR 0021).
//!
//! The game's camera already goes around the character on a sphere: an
//! angle sideways, an elevation and a distance, from which one tail of its
//! function works out where the camera is, pushes it out of walls and
//! turns it to the character. What the original decides by itself is the
//! two angles (turning behind the character, swinging away from walls,
//! coming back to a rest elevation). This replaces that decision and
//! nothing else: the angles are the player's, and the game's own tail
//! places the camera.

use crate::controls::camera_actions;
use crate::{Machine, Stop};

/// In the function of the camera that follows (`0x4045E0`): where it has
/// eased the distance and starts deciding the angles, and its tail, which
/// places the camera from them.
pub const ANGLES_DECIDED: u32 = 0x0040_46CE;
pub const CAMERA_PLACED: u32 = 0x0040_53B1;
/// The stretch's second instruction, and what its first loads into `eax`.
pub const ANGLES_DECIDED_BY_THE_GAME: u32 = 0x0040_46D3;
const SPEED_SIDEWAYS: u32 = 0x0052_F368;

/// The pause menu (`0x49F4B0` reads it, `0x49FD40` draws it): its page,
/// the entry the pointer is at, the entries each page has; the camera's
/// page, and the option its two entries set.
const MENU_PAGE: u32 = 0x0052_ADB0;
const MENU_ENTRY: u32 = 0x0052_B7E4;
const MENU_ENTRIES: u32 = 0x0050_39BC;
const CAMERA_PAGE: u32 = 1;
const OPTIONS: u32 = 0x0052_F0CC;
const CAMERA_TURNS_BY_ITSELF: u32 = 0x40;
/// Three stretches of the menu: where it takes a page's number of
/// entries, where the camera's page takes the choice, where it is drawn.
pub const ENTRIES_COUNTED: u32 = 0x0049_F518;
pub const ENTRIES_COUNT_TAKEN: u32 = 0x0049_F520;
pub const CAMERA_CHOSEN: u32 = 0x0049_F6C5;
pub const CAMERA_CHOICE_TAKEN: u32 = 0x0049_F6FC;
pub const CAMERA_PAGE_DRAWN: u32 = 0x004A_0132;
pub const CAMERA_PAGE_DONE: u32 = 0x004A_0179;
/// The menu's text function: height, text, three shades, which sheet.
const MENU_TEXT: u32 = 0x0040_1FB0;
const CAMERA_TITLE: u32 = 0x0050_2714;
const CAMERA_NAMES: [u32; 2] = [0x0050_2720, 0x0050_2730];
const TITLE_HEIGHT: u32 = 0x54;
/// The page's three entries: the menu's first page has four at this step.
const ENTRY_HEIGHTS: [u32; 3] = [0x60, 0x68, 0x70];

/// In place of the two instructions that take a page's number of entries
/// (`ebp` holds the page; the number goes to `ecx`): the camera's page
/// has three.
pub fn count_camera_entries(m: &mut Machine) -> Result<(), Stop> {
    let page = m.cpu.regs[5];
    m.cpu.regs[1] =
        if page == CAMERA_PAGE { 3 } else { m.memory.read(MENU_ENTRIES.wrapping_add(page), 1).unwrap_or(0) };
    m.cpu.instructions += 2;
    Ok(())
}

/// In place of the stretch that takes the choice on the camera's page
/// (`dx` holds the entry, `ecx` is 1 when the page was cancelled): the
/// game's two cameras as the original sets them, and the third; then back
/// to the first page.
pub fn choose_camera(m: &mut Machine) -> Result<(), Stop> {
    let (entry, cancelled) = (m.cpu.regs[2] & 0xFFFF, m.cpu.regs[1] != 0);
    if !cancelled {
        let options = m.memory.read(OPTIONS, 1).unwrap_or(0);
        match entry {
            0 => m.memory.write(OPTIONS, 1, options & !CAMERA_TURNS_BY_ITSELF),
            1 => m.memory.write(OPTIONS, 1, options | CAMERA_TURNS_BY_ITSELF),
            _ => Some(()),
        };
        m.os.controls.camera_orbits = entry == 2;
        // Which of the three it is, is the project's to remember: the
        // game's saved block has a bit for its own two.
        m.os.keep_settings();
    }
    m.set_game_half(MENU_PAGE, 0)?;
    m.set_game_half(MENU_ENTRY, 0)?;
    Ok(())
}

/// In place of the stretch that draws the camera's page: its title and
/// three entries, each in the shades the menu has worked out for its
/// place (the entry the pointer is at pulses).
pub fn draw_camera_page(m: &mut Machine) -> Result<(), Stop> {
    let esp = m.cpu.regs[4];
    let own = m.os.controls.orbit_camera_text.clone();
    let third = m.os.kept_string(&mut m.memory, &own);
    m.call_guest_c(MENU_TEXT, &[TITLE_HEIGHT, CAMERA_TITLE, 0x80, 0x80, 0, 0])?;
    for (place, text) in CAMERA_NAMES.into_iter().chain([third]).enumerate() {
        let at = 4 * place as u32;
        let shades =
            [m.game_word(esp + 0x44 + at)?, m.game_word(esp + 0x34 + at)?, m.game_word(esp + 0x24 + at)?];
        m.call_guest_c(MENU_TEXT, &[ENTRY_HEIGHTS[place], text, shades[0], shades[1], shades[2], 0])?;
    }
    Ok(())
}

/// The game's time step of the frame.
const STEP: u32 = 0x0052_F2D4;
/// Sines, 16 bits each, 4096 to the turn, 16384 for one.
const SINES: u32 = 0x004F_E788;
/// The distance the camera keeps when nothing is in the way.
const DISTANCE_WANTED: u32 = 0x0050_A128;
/// Raised while the character is in a state that lifts the view.
const VIEW_RAISED: u32 = 0x0050_A4E0;
/// Prepares the level's collision around a segment, and casts a sphere
/// along one: start, movement (cut short at what is hit), which surfaces,
/// a flag, the sphere's radius.
const COLLISION_AROUND: u32 = 0x0048_A4C0;
const CAST: u32 = 0x0048_C860;
const SURFACES_THE_CAMERA_MEETS: u32 = 0x8000;
const CAMERA_RADIUS: u32 = 200;

/// The camera's own numbers, from the start of its structure.
const HEIGHT_FOLLOWED: u32 = 0x18;
const HEIGHT_LOOKED_AT: u32 = 0x1C;
const DISTANCE: u32 = 0x26;
const YAW: u32 = 0x28;
const ELEVATION: u32 = 0x2E;

/// What the camera's actions turn it by, in a unit of the step: the
/// original's, in its camera that does not turn by itself.
const KEY_TURN: i32 = 16;
/// The player's view goes from looking steeply up to looking down from
/// almost straight above (1024 is a quarter turn). The camera itself goes
/// no lower than a little above the ground the character stands on: at
/// its rest it is 256 units above the height it looks at, and 1200 away.
/// Below that the view is tilted instead, upwards past the character: the
/// camera stays where it is and no longer looks straight at him. (The
/// original's own camera stays within -0x200 and 0x300 and always looks
/// at the character.)
const VIEW_LOWEST: i32 = -0x300;
const CAMERA_LOWEST: i32 = -0x60;
const HIGHEST: i32 = 0x380;
/// The camera's rotation about the axis across the view, 4096 to the turn.
const ROTATION_UP_AND_DOWN: u32 = 0x20;
/// Which way that rotation goes when the view is tilted up.
const TILT_UP: i32 = -1;
/// The `ret` of the follower's function, and its camera.
pub const CAMERA_FOLLOWED: u32 = 0x0040_5825;
const FOLLOWER: u32 = 0x0052_F3A0;

/// In place of the `ret` of the follower's function: the view is tilted
/// up by what the player asked for beyond the lowest place of the camera.
/// The tilt is taken off again before the function next runs, which eases
/// this rotation from its own last value.
pub fn tilt_view(m: &mut Machine) -> Result<(), Stop> {
    m.cpu.instructions += 1;
    let tilt = m.os.controls.tilt;
    if m.os.controls.camera_orbits && tilt != 0 {
        let rotation = m.game_half(FOLLOWER + ROTATION_UP_AND_DOWN)? as i32;
        m.set_game_half(FOLLOWER + ROTATION_UP_AND_DOWN, (rotation + TILT_UP * tilt) as u32 & 0xFFF)?;
        m.os.controls.tilt_applied = TILT_UP * tilt;
    }
    Ok(())
}

/// In the visor's function (`0x4038E0`): where every path arrives with the
/// two rates it turns the aim by, and the instruction after the two this
/// stands for.
pub const VISOR_TURNS: u32 = 0x0040_3FC1;
pub const VISOR_TURNED: u32 = 0x0040_3FC6;
/// Where the character faces: the visor looks that way.
const HEADING: u32 = 0x0052_F30E;
/// In the visor the camera's wanted place holds the aim's elevation
/// instead, 0 for level; a quarter turn more, it is kept within these.
const AIM_ELEVATION: u32 = 0x0C;
const AIM_LIMITS: (i32, i32) = (0xC8, 0x720);

/// A joypad's second stick at rest: the game's own measure for its first.
const STICK_AT_REST: i32 = 0x1800;

/// What the player turns the view by in this frame, sideways and up or
/// down, as angles: the mouse's movement, and a joypad's second stick
/// held for the frame's time step.
fn player_turn(m: &mut Machine, step: i32) -> [i32; 2] {
    let motion = m.os.input.frame_motion;
    let stick = m.os.input.pad.map_or([0; 2], |pad| [pad.axes[3] as i32, pad.axes[4] as i32]);
    let controls = &mut m.os.controls;
    let mut turned = [0; 2];
    for axis in 0..2 {
        // Past its rest the stick turns in proportion, up to its rate.
        let pushed = stick[axis].signum() * (stick[axis].abs() - STICK_AT_REST).max(0);
        let by_stick = pushed * controls.stick_turn[axis] * step / (0x8000 - STICK_AT_REST);
        let parts = motion[axis] * controls.turn[axis] + by_stick + controls.turn_left_over[axis];
        turned[axis] = parts.div_euclid(256);
        controls.turn_left_over[axis] = parts.rem_euclid(256);
    }
    if controls.inverted {
        turned[1] = -turned[1];
    }
    turned
}

/// In place of two instructions of the visor, where it is about to turn
/// the aim by its two rates: the mouse's movement turns it first, straight
/// and not through the rates. `ebp` holds the camera, `ebx` the time step,
/// `esi` the rate sideways; it leaves their product in `eax`, as the two
/// instructions do.
pub fn aim_visor(m: &mut Machine) -> Result<(), Stop> {
    let camera = m.cpu.regs[5];
    // The visor turns the camera itself: no tilt of the follower's is in it.
    m.os.controls.tilt_applied = 0;
    let step = m.cpu.regs[3] as i32;
    let [sideways, down] = player_turn(m, step);
    if sideways != 0 {
        let heading = m.game_half(HEADING)? as i32;
        m.set_game_half(HEADING, (heading + sideways) as u32 & 0xFFF)?;
    }
    if down != 0 {
        let elevation = (m.game_word(camera + AIM_ELEVATION)? as i32 + 0x400) & 0xFFF;
        let elevation = (elevation + down * VISOR_DOWN).clamp(AIM_LIMITS.0, AIM_LIMITS.1);
        m.set_game_word(camera + AIM_ELEVATION, (elevation - 0x400) as u32 & 0xFFF)?;
    }
    m.cpu.regs[0] = m.cpu.regs[3].wrapping_mul(m.cpu.regs[6]);
    m.cpu.instructions += 2;
    Ok(())
}

/// Which way the aim's elevation goes when the mouse moves down.
const VISOR_DOWN: i32 = 1;

fn sine(m: &mut Machine, angle: i32) -> Result<i32, Stop> {
    Ok(m.game_half(SINES + 2 * (angle & 0xFFF) as u32)? as u16 as i16 as i32)
}

/// In place of the stretch of the camera's function that decides its two
/// angles. `esi` holds the camera; the character's place is in the
/// function's own variables. It leaves what the stretch leaves: `ebx`
/// saved on the stack, and the two marks that make the tail put the
/// camera exactly where the angles say and keep it out of walls.
pub fn orbit(m: &mut Machine) -> Result<(), Stop> {
    if !m.os.controls.camera_orbits {
        // One of the game's own cameras is chosen: the stretch's first
        // instruction, and the game's code goes on from the next.
        m.cpu.regs[0] = m.game_word(SPEED_SIDEWAYS)?;
        m.cpu.instructions += 1;
        (m.os.controls.tilt, m.os.controls.tilt_applied) = (0, 0);
        m.resume_at = Some(ANGLES_DECIDED_BY_THE_GAME);
        return Ok(());
    }
    // The stretch begins by saving `ebx`.
    let esp = m.cpu.regs[4] - 4;
    m.set_game_word(esp, m.cpu.regs[3])?;
    m.cpu.regs[4] = esp;
    let camera = m.cpu.regs[6];
    let step = m.game_word(STEP)? as i32;
    let (x, y, z) = (m.game_word(esp + 0x1C)?, m.game_word(esp + 0x10)? as i32, m.game_word(esp + 0x18)?);

    // The angles: the mouse's movement of this frame, and the camera's two
    // actions as the original's own camera takes them.
    let turned = player_turn(m, step);
    let (left, right) = camera_actions(m)?;
    let keys = KEY_TURN * step * (left as i32 - right as i32);
    let yaw = (m.game_half(camera + YAW)? as i32 + turned[0] + keys) & 0xFFF;
    // The tilt given to the view at the end of the last frame comes off,
    // so that the tail eases the rotation from its own value.
    let applied = std::mem::take(&mut m.os.controls.tilt_applied);
    if applied != 0 {
        let rotation = m.game_half(camera + ROTATION_UP_AND_DOWN)? as i32;
        m.set_game_half(camera + ROTATION_UP_AND_DOWN, (rotation - applied) as u32 & 0xFFF)?;
    }
    // The view the player wants goes on from the camera's elevation unless
    // the game has put the camera elsewhere since (a reset, a script).
    let found = m.game_half(camera + ELEVATION)? as u16 as i16 as i32;
    let controls = &mut m.os.controls;
    if controls.elevation_left != Some(found) {
        controls.view = found;
    }
    controls.view = (controls.view + turned[1]).clamp(VIEW_LOWEST, HIGHEST);
    let elevation = controls.view.max(CAMERA_LOWEST);
    controls.tilt = elevation - controls.view;
    controls.elevation_left = Some(elevation);
    m.set_game_half(camera + YAW, yaw as u32)?;
    m.set_game_half(camera + ELEVATION, elevation as u32 & 0xFFFF)?;

    // The height looked at comes after the character's as in the original.
    let mut followed = m.game_word(camera + HEIGHT_FOLLOWED)? as i32;
    followed -= (followed - y + 0x1000) >> 5;
    m.set_game_word(camera + HEIGHT_FOLLOWED, followed as u32)?;
    let mut looked_at = m.game_word(camera + HEIGHT_LOOKED_AT)? as i32;
    looked_at -= ((looked_at - followed) * step) / 16;
    let raised = m.game_word(VIEW_RAISED)? as i32;
    if raised != 0 {
        looked_at = y - ((y - looked_at - 0x1000) / 256) * (0x100 - raised) - 0x1000;
    }
    m.set_game_word(camera + HEIGHT_LOOKED_AT, looked_at as u32)?;

    // What is between the character's head and the camera's place at its
    // full distance brings the camera nearer, as in the original.
    let wanted = m.game_word(DISTANCE_WANTED)? as i32;
    let level = (sine(m, elevation + 0x400)? * wanted) >> 14;
    let rise = (sine(m, elevation - 0x800)? * wanted) >> 9;
    let movement =
        [-((sine(m, yaw)? * level) >> 9), looked_at - y + rise, -((sine(m, yaw + 0x400)? * level) >> 9)];
    let vectors = esp - 0x40;
    let (start, moved) = (vectors, vectors + 12);
    for (at, value) in [x, (y - 0x1F00) as u32, z].into_iter().enumerate() {
        m.set_game_word(start + 4 * at as u32, value)?;
    }
    for (at, value) in movement.into_iter().enumerate() {
        m.set_game_word(moved + 4 * at as u32, value as u32)?;
    }
    m.cpu.regs[4] = vectors - 0x10;
    let [by_x, by_y, by_z] = movement.map(|by| by as u32);
    m.call_guest_c(COLLISION_AROUND, &[start, by_x, by_y, by_z, 0x7D0])?;
    let hit = m.call_guest_c(CAST, &[start, moved, SURFACES_THE_CAMERA_MEETS, 1, CAMERA_RADIUS])?;
    m.cpu.regs[4] = esp;
    if hit != 0 {
        let mut squares = 0i64;
        for at in 0..3 {
            let part = (m.game_word(moved + 4 * at)? as i32 >> 5) as i64;
            squares += part * part;
        }
        let reached = ((squares as f64).sqrt() as i32).max(10);
        m.set_game_half(camera + DISTANCE, reached as u32 & 0xFFFF)?;
    }

    // The tail takes these two marks as "the camera is put, not eased":
    // exactly where the angles say, turned straight to the character, and
    // pushed out of whatever it would be inside.
    m.set_game_word(esp + 0x20, 1)?;
    m.set_game_word(esp + 0x24, 1)?;
    Ok(())
}
