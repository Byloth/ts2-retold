//! Known builds of the game executable, identified by hash.
//!
//! See `docs/adr/0003-reference-copy.md`: the baseline is the unmodified retail
//! executable, and everything the recompiler knows (addresses, patches) is
//! tied to one exact build.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build {
    pub id: &'static str,
    pub sha256: &'static str,
    pub description: &'static str,
    /// False while the hash has not been compared with an original disc.
    pub confirmed_retail: bool,
}

pub const KNOWN_BUILDS: &[Build] = &[Build {
    id: "pc-1999-10-29",
    sha256: "023eb6a9459443b34d24cf685591bfeb3b95e1acf579405f6d8fa4407ccbdaf0",
    description: "PC, linked 1999-10-29, English text",
    confirmed_retail: false,
}];

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn identify(executable: &[u8]) -> Option<&'static Build> {
    let hash = sha256_hex(executable);
    KNOWN_BUILDS.iter().find(|b| b.sha256 == hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_rejects_unknown_input() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(identify(b"not the game").is_none());
    }
}
