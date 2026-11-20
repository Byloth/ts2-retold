//! MPEG-1 system streams: what the game's movies are.
//!
//! [`demux`] splits a file into its video and audio streams, [`video`]
//! decodes the pictures. Everything is integer arithmetic in a fixed
//! order, so a movie decodes to the same pixels on every machine.
//!
//! The decoder is a port to Rust of pl_mpeg by Dominic Szablewski
//! (<https://github.com/phoboslab/pl_mpeg>, MIT license, see
//! `LICENSE-pl_mpeg` in this crate): the same algorithms and tables,
//! restructured to work on a whole stream held in memory.

mod bits;
pub mod demux;
mod tables;
pub mod video;
