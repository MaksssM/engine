use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// A globally unique identifier for every persistent engine object.
///
/// Two random u64 halves give 128-bit space (collision-free in practice).
/// Deterministic `from_parts` lets scenes store stable ids across sessions.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Uid {
    pub hi: u64,
    pub lo: u64,
}

impl Uid {
    /// Generate a new unique id using a simple counter + timestamp mix.
    /// Not cryptographically random, but collision-free within one process.
    pub fn new() -> Self {
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self { hi: time, lo: seq }
    }

    pub const fn from_parts(hi: u64, lo: u64) -> Self {
        Self { hi, lo }
    }

    pub const INVALID: Self = Self { hi: 0, lo: 0 };

    pub fn is_valid(self) -> bool {
        self != Self::INVALID
    }
}

impl Default for Uid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uid({:016x}:{:016x})", self.hi, self.lo)
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}{:016x}", self.hi, self.lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_are_unique() {
        let a = Uid::new();
        let b = Uid::new();
        assert_ne!(a, b);
    }

    #[test]
    fn uid_from_parts_round_trips() {
        let uid = Uid::from_parts(0xDEAD, 0xBEEF);
        assert_eq!(uid.hi, 0xDEAD);
        assert_eq!(uid.lo, 0xBEEF);
        assert!(uid.is_valid());
        assert!(!Uid::INVALID.is_valid());
    }

    #[test]
    fn uid_display_format() {
        let uid = Uid::from_parts(0x1234, 0x5678);
        let s = uid.to_string();
        assert_eq!(s.len(), 32);
    }

    #[test]
    fn uid_serde_round_trip() {
        let uid = Uid::from_parts(123456, 789012);
        let ron = ron::to_string(&uid).unwrap();
        let back: Uid = ron::from_str(&ron).unwrap();
        assert_eq!(uid, back);
    }
}
