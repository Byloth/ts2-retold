//! MPEG-1 system streams: what the game's movies are.
//!
//! [`demux`] splits a file into its video and audio streams, [`video`]
//! decodes the pictures and [`audio`] the sound. The pictures are integer
//! arithmetic throughout and the sound's floating-point sums are done in
//! a fixed order, so a movie decodes to the same pixels and samples on
//! every machine.
//!
//! The decoder is a port to Rust of pl_mpeg by Dominic Szablewski
//! (<https://github.com/phoboslab/pl_mpeg>, MIT license, see
//! `LICENSE-pl_mpeg` in this crate): the same algorithms and tables,
//! restructured to work on a whole stream held in memory. Its audio
//! decoder is based on kjmp2 by Martin J. Fiedler (zlib license, see
//! `LICENSE-kjmp2`), and differs from both in how samples are requantised
//! (see [`audio`]).

pub mod audio;
mod audio_tables;
mod bits;
pub mod demux;
mod tables;
pub mod video;
