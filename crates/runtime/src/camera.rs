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
/// How low and how high the camera goes: the original's own camera stays
/// within -0x200 and 0x300 (1024 is straight above).
const LOWEST: i32 = -0x280;
const HIGHEST: i32 = 0x380;

fn sine(m: &mut Machine, angle: i32) -> Result<i32, Stop> {
    Ok(m.game_half(SINES + 2 * (angle & 0xFFF) as u32)? as u16 as i16 as i32)
}

/// In place of the stretch of the camera's function that decides its two
/// angles. `esi` holds the camera; the character's place is in the
/// function's own variables. It leaves what the stretch leaves: `ebx`
/// saved on the stack, and the two marks that make the tail put the
/// camera exactly where the angles say and keep it out of walls.
pub fn orbit(m: &mut Machine) -> Result<(), Stop> {
    // The stretch begins by saving `ebx`.
    let esp = m.cpu.regs[4] - 4;
    m.set_game_word(esp, m.cpu.regs[3])?;
    m.cpu.regs[4] = esp;
    let camera = m.cpu.regs[6];
    let step = m.game_word(STEP)? as i32;
    let (x, y, z) = (m.game_word(esp + 0x1C)?, m.game_word(esp + 0x10)? as i32, m.game_word(esp + 0x18)?);

    // The angles: the mouse's movement of this frame, and the camera's two
    // actions as the original's own camera takes them.
    let motion = m.os.input.frame_motion;
    let controls = &mut m.os.controls;
    let mut turned = [0; 2];
    for axis in 0..2 {
        let parts = motion[axis] * controls.turn[axis] + controls.turn_left_over[axis];
        turned[axis] = parts.div_euclid(256);
        controls.turn_left_over[axis] = parts.rem_euclid(256);
    }
    if controls.inverted {
        turned[1] = -turned[1];
    }
    let (left, right) = camera_actions(m)?;
    let keys = KEY_TURN * step * (left as i32 - right as i32);
    let yaw = (m.game_half(camera + YAW)? as i32 + turned[0] + keys) & 0xFFF;
    let elevation = (m.game_half(camera + ELEVATION)? as u16 as i16 as i32 + turned[1]).clamp(LOWEST, HIGHEST);
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
    let movement = [
        -((sine(m, yaw)? * level) >> 9),
        looked_at - y + rise,
        -((sine(m, yaw + 0x400)? * level) >> 9),
    ];
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
