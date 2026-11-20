//! Addresses inside the game executable that the runtime's modifications
//! touch. They are valid for build `pc-1999-10-29` only (see
//! `andy_recomp::builds`); each is documented with how it was established.

/// Index of the display driver chosen in the "Screen Mode Select" window and
/// saved in `toy2.cfg`. A negative value makes that window (at `0x432327`)
/// ignore the saved device and mode indices and start from its defaults: the
/// first hardware 3D device and the first screen mode at least 480 lines
/// high. The executable initialises it to -1 (at `0x4CE774`).
pub const SAVED_DISPLAY_DRIVER: u32 = 0x0050_8D7C;
