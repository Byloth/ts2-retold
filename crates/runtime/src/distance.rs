//! Things drawn from farther away than the original draws them
//! (`draw-distance`, ADR 0022).
//!
//! The game stops drawing scenery, objects and coins a short way from the
//! camera, a limit that suited the machines of its time. Each kind has
//! its own test, and only tests that decide drawing alone are touched:
//! what the game's logic reads stays what the original computes.
//!
//! Addresses are the reference build's; how they were found is in
//! `docs/CODE-DISCOVERY.md`.

use crate::{Machine, Stop};

/// The function that sets the scenery's limits for the frame from the row
/// of the detail chosen (one argument: the detail, 0 to 2).
pub const SCENERY_LIMITS: u32 = 0x004C_DD10;
/// Three rows of six numbers (32-bit floating point): the depth the far,
/// simplified layer starts at; the depth the near, detailed layer goes
/// to; the near planes of the two passes; the distance all round within
/// which anything is drawn; the far layer's inner radius.
const DETAIL_ROWS: u32 = 0x0050_8D28;
/// Where the function puts the first four; the last two go to this
/// function of the game's.
const FAR_LAYER_FROM: u32 = 0x0050_8D14;
const NEAR_LAYER_TO: u32 = 0x0050_8D18;
const NEAR_PLANES: [u32; 2] = [0x0050_8D08, 0x0050_8D10];
const SET_RADIAL_LIMITS: u32 = 0x004B_C410;
/// The world's far plane: nothing is drawn beyond it whatever the limits.
const FAR_PLANE: f32 = 48000.0;

/// In the loop of the draw function that goes through the coins and
/// pickup marks: where it has the square of an entry's distance from the
/// camera (less 100) to compare with the square of the limit, and the two
/// places it goes on at, drawing the entry or leaving it out.
pub const COIN_TESTED: u32 = 0x0044_13A4;
pub const COIN_DRAWN: u32 = 0x0044_13CB;
pub const COIN_LEFT_OUT: u32 = 0x0044_161C;
/// In the loop that picks coins up: where it asks whether the entry was
/// drawn (bit 0x80 of the byte at 0x0D), and the two places it goes on at.
pub const COIN_PICKABLE: u32 = 0x004A_11CC;
pub const COIN_CONSIDERED: u32 = 0x004A_11D8;
pub const COIN_PASSED_OVER: u32 = 0x004A_1694;
const COIN_FLAGS: u32 = 0x0D;
const COIN_WAS_DRAWN: u32 = 0x80;

/// A number of the game's, 32-bit floating point.
fn float(m: &mut Machine, at: u32) -> Result<f32, Stop> {
    Ok(f32::from_bits(m.game_word(at)?))
}

/// In place of the function that sets the scenery's limits: the depths at
/// which the detailed layer ends and the simplified one begins, and the
/// distance all round, are those of the detail chosen times the factor,
/// up to the world's far plane. The near planes are the original's.
pub fn set_scenery_limits(m: &mut Machine) -> Result<(), Stop> {
    let esp = m.cpu.regs[4];
    let detail = (m.game_word(esp + 4)? as i32).clamp(0, 2) as u32;
    let row = DETAIL_ROWS + 24 * detail;
    let factor = m.os.draw_distance.max(1) as f32;
    let farther = |limit: f32| (limit * factor).min(FAR_PLANE);
    let (far_from, near_to) = (float(m, row)?, float(m, row + 4)?);
    let (radial, inner) = (float(m, row + 16)?, m.game_word(row + 20)?);
    m.call_guest_c(SET_RADIAL_LIMITS, &[farther(radial).to_bits(), inner])?;
    m.set_game_word(FAR_LAYER_FROM, farther(far_from).to_bits())?;
    m.set_game_word(NEAR_LAYER_TO, farther(near_to).to_bits())?;
    for (at, plane) in NEAR_PLANES.into_iter().zip([row + 8, row + 12]) {
        let plane = m.game_word(plane)?;
        m.set_game_word(at, plane)?;
    }
    Ok(())
}

/// In place of the test of a coin's distance in the draw loop (`edx` and
/// `eax` hold the squares of the two differences, `ecx` the square of the
/// limit, `esi` the entry): the coin is drawn within the limit times the
/// factor, fading in over the same margin as the original's. Whether the
/// original would have drawn it is remembered for the pickup loop.
pub fn test_coin(m: &mut Machine) -> Result<(), Stop> {
    let (across, along, limit) = (m.cpu.regs[2] as i32, m.cpu.regs[0] as i32, m.cpu.regs[1] as i32);
    let entry = m.cpu.regs[6];
    // The original's own arithmetic, 32 bits wide.
    let measured = across.wrapping_add(along).wrapping_sub(100);
    if measured < limit {
        m.os.coins_within_reach.insert(entry);
    } else {
        m.os.coins_within_reach.remove(&entry);
    }
    let factor = m.os.draw_distance.max(1) as i64;
    let left = limit as i64 * factor * factor - measured as i64;
    if left <= 0 {
        m.cpu.regs[0] = measured as u32;
        m.resume_at = Some(COIN_LEFT_OUT);
        m.cpu.instructions += 3;
    } else {
        // Its opacity: full from 0x4000 inside the limit, as the original.
        m.cpu.regs[5] = (left >> 6).min(0xFF) as u32;
        m.cpu.instructions += 10;
    }
    Ok(())
}

/// In place of the pickup loop's question whether a coin was drawn (`esi`
/// holds the entry; the answer's byte goes to `bl`): it must also have
/// been within the original's limit, so that coins are picked up exactly
/// where the original picks them up.
pub fn coin_is_pickable(m: &mut Machine) -> Result<(), Stop> {
    let entry = m.cpu.regs[6];
    let flags = m.memory.read(entry + COIN_FLAGS, 1).unwrap_or(0);
    m.cpu.regs[3] = m.cpu.regs[3] & !0xFF | flags;
    if flags & COIN_WAS_DRAWN == 0 || !m.os.coins_within_reach.contains(&entry) {
        m.resume_at = Some(COIN_PASSED_OVER);
    }
    m.cpu.instructions += 3;
    Ok(())
}

/// Enemies and characters: 64 places of 0x9C bytes. Of each: where it is
/// (three numbers, 32 to a unit of distance), its type, its radius, the
/// range within four times which it is seen, its room, and its state
/// (above zero while it is alive).
const OBJECTS: u32 = 0x0052_C840;
const OBJECT_COUNT: u32 = 64;
const OBJECT_SIZE: u32 = 0x9C;
const OBJECT_TYPE: u32 = 0x14;
const OBJECT_RADIUS: u32 = 0x3E;
const OBJECT_FLAGS: u32 = 0x40;
const OBJECT_RANGE: u32 = 0x42;
const OBJECT_ROOM: u32 = 0x6C;
const OBJECT_STATE: u32 = 0x7E;
/// Those the game updates in this frame, a list of addresses ended by 0;
/// the list it draws from, made of that one and the character; the
/// character.
const UPDATED: u32 = 0x0052_F1D0;
const DRAWN: u32 = 0x0052_9D48;
const CHARACTER: u32 = 0x0052_F300;
/// A type's model, by its number: none if the word it points to is 0.
const MODELS: u32 = 0x0054_7CD4;
/// A byte for each room: not 0 if the scenery's walk drew it.
const ROOMS_DRAWN: u32 = 0x00A4_C414;
/// Whether a sphere (its centre as three floats, its radius) is in view:
/// two bits a plane, the lower set where it is wholly outside.
const SPHERE_IN_VIEW: u32 = 0x004B_A1F0;
const OUTSIDE_A_PLANE: u32 = 0x5555_5555;

/// The `ret` of the function that says which of the updated objects are
/// seen (`0x447BD0`; its argument is the camera).
pub const SEEN_DECIDED: u32 = 0x0044_7D20;
/// The function that makes the list to draw from.
pub const DRAW_LIST_MADE: u32 = 0x004A_28B0;
/// In the function that draws that list: where it asks whether an entry
/// is seen (`eax` holds the entry's place in the list), and the two
/// places it goes on at, drawing it or passing it over.
pub const ENTRY_ASKED: u32 = 0x004C_DC5C;
pub const ENTRY_DRAWN: u32 = 0x004C_DC63;
pub const ENTRY_PASSED_OVER: u32 = 0x004C_DCBD;

fn signed_half(m: &mut Machine, at: u32) -> Result<i32, Stop> {
    Ok(m.game_half(at)? as u16 as i16 as i32)
}

/// In place of the `ret` of the function that says which objects are
/// seen: the objects the game does not update, because they are beyond
/// its range, are looked at too, with the game's own conditions (alive,
/// with a model, in a room that was drawn, in view) and the range times
/// the factor. Those that pass are drawn in this frame, as they stand:
/// nothing of them is updated, and the game's own marks are not touched.
pub fn find_far_objects(m: &mut Machine) -> Result<(), Stop> {
    m.cpu.instructions += 1;
    let esp = m.cpu.regs[4];
    let camera = m.game_word(esp + 4)?;
    let mut updated = Vec::new();
    while let entry @ 1.. = m.game_word(UPDATED + 4 * updated.len() as u32)? {
        updated.push(entry);
    }
    let factor = m.os.draw_distance.max(1) as i64;
    // The matrices of what is drawn have room for 64 entries.
    let room_left = (OBJECT_COUNT as usize - 1).saturating_sub(updated.len());
    let mut far = Vec::new();
    for number in 0..OBJECT_COUNT {
        let object = OBJECTS + number * OBJECT_SIZE;
        if far.len() == room_left || updated.contains(&object) {
            continue;
        }
        let kind = signed_half(m, object + OBJECT_TYPE)?;
        if kind <= 0 || signed_half(m, object + OBJECT_STATE)? <= 0 {
            continue;
        }
        let model = m.game_word(MODELS + 4 * kind as u32)?;
        if m.game_half(model)? == 0 {
            continue;
        }
        let room = m.memory.read(object + OBJECT_ROOM, 1).unwrap_or(0x80) as u8 as i8;
        if room < 0 || m.memory.read(ROOMS_DRAWN + (room as u32 & 0x3F), 1).unwrap_or(0) == 0 {
            continue;
        }
        let mut place = [0i32; 3];
        let mut squares = 0i64;
        for (axis, place) in place.iter_mut().enumerate() {
            *place = m.game_word(object + 4 * axis as u32)? as i32;
            let apart = (m.game_word(camera + 4 * axis as u32)? as i32).wrapping_sub(*place) >> 5;
            squares += apart as i64 * apart as i64;
        }
        let range = signed_half(m, object + OBJECT_RANGE)? as i64 * 4 * factor;
        if squares >= range * range {
            continue;
        }
        // The sphere's centre in units of distance, as the game gives it.
        let centre = esp - 0x20;
        for (axis, place) in place.into_iter().enumerate() {
            m.set_game_word(centre + 4 * axis as u32, ((place / 32) as f32).to_bits())?;
        }
        let radius = (signed_half(m, object + OBJECT_RADIUS)? as f32).to_bits();
        m.cpu.regs[4] = centre - 0x10;
        let outside = m.call_guest_c(SPHERE_IN_VIEW, &[centre, radius])?;
        m.cpu.regs[4] = esp;
        if outside & OUTSIDE_A_PLANE == 0 {
            far.push(object);
        }
    }
    m.os.far_objects = far;
    Ok(())
}

/// In place of the function that makes the list to draw from: the
/// objects updated, then those seen from afar, then the character.
pub fn make_draw_list(m: &mut Machine) -> Result<(), Stop> {
    let mut count = 0;
    while let entry @ 1.. = m.game_word(UPDATED + 4 * count)? {
        m.set_game_word(DRAWN + 4 * count, entry)?;
        count += 1;
    }
    for object in m.os.far_objects.clone() {
        m.set_game_word(DRAWN + 4 * count, object)?;
        count += 1;
    }
    m.set_game_word(DRAWN + 4 * count, CHARACTER)?;
    m.set_game_word(DRAWN + 4 * count + 4, 0)?;
    Ok(())
}

/// In place of the question whether an entry of the list is seen: one
/// seen from afar is drawn too.
pub fn entry_is_drawn(m: &mut Machine) -> Result<(), Stop> {
    let object = m.game_word(m.cpu.regs[0])?;
    m.cpu.regs[1] = object;
    let seen = m.memory.read(object + OBJECT_FLAGS, 1).unwrap_or(0) & 1 != 0;
    if !seen && !m.os.far_objects.contains(&object) {
        m.resume_at = Some(ENTRY_PASSED_OVER);
    }
    m.cpu.instructions += 3;
    Ok(())
}
