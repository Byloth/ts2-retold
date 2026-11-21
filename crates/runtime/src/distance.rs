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
