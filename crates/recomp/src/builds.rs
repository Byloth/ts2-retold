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
    /// Whether the hash is that of the reference copy of the game (ADR 0003).
    pub confirmed_retail: bool,
}

/// The reference build, which everything else is tied to.
pub const REFERENCE: &str = "pc-1999-10-29";
/// The Italian build, the source of the Italian language (ADR 0016).
pub const ITALIAN: &str = "pc-2000-02-16-it";

pub const KNOWN_BUILDS: &[Build] = &[
    Build {
        id: REFERENCE,
        sha256: "023eb6a9459443b34d24cf685591bfeb3b95e1acf579405f6d8fa4407ccbdaf0",
        description: "PC, linked 1999-10-29, English text",
        confirmed_retail: true,
    },
    Build {
        id: ITALIAN,
        sha256: "e46abf778d2c532883000d313c1ca383130389ba704266c5190ced7f2cfbfeb5",
        description: "PC, linked 2000-02-16, Italian text; not run itself, its language is brought to the \
                      reference build",
        confirmed_retail: false,
    },
];

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
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
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(identify(b"not the game").is_none());
    }
}
