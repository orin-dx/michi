//! `StepId` and `RunId`: short, validated, caller-chosen identifiers.

use std::fmt;

/// Longest accepted id, in bytes (ids are ASCII, so also in characters).
pub const MAX_ID_LEN: usize = 64;

/// Why a string is not a valid [`StepId`] or [`RunId`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// The id is empty.
    #[error("id must not be empty")]
    Empty,
    /// The id is longer than [`MAX_ID_LEN`].
    #[error("id is {len} bytes long, the maximum is {max}")]
    TooLong {
        /// The id's length in bytes.
        len: usize,
        /// The maximum length.
        max: usize,
    },
    /// The id contains a character outside the slug charset.
    #[error("id has {ch:?} at byte {index}; use ASCII letters, digits, '_', '-' or '.', starting with a letter, digit or '_'")]
    InvalidChar {
        /// The offending character.
        ch: char,
        /// Its byte offset.
        index: usize,
    },
}

/// Checks the slug rule shared by [`StepId`] and [`RunId`].
fn validate(s: &str) -> Result<(), IdError> {
    if s.is_empty() {
        return Err(IdError::Empty);
    }
    if s.len() > MAX_ID_LEN {
        return Err(IdError::TooLong { len: s.len(), max: MAX_ID_LEN });
    }
    for (index, ch) in s.char_indices() {
        let ok = ch.is_ascii_alphanumeric() || ch == '_' || (index > 0 && (ch == '-' || ch == '.'));
        if !ok {
            return Err(IdError::InvalidChar { ch, index });
        }
    }
    Ok(())
}

macro_rules! slug_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
        pub struct $name(String);

        impl $name {
            /// Validates `id` against the slug rule: 1 to 64 ASCII letters, digits, `_`, `-` or `.`, starting with a
            /// letter, digit or `_`.
            ///
            /// # Errors
            ///
            /// [`IdError::Empty`], [`IdError::TooLong`] or [`IdError::InvalidChar`] when `id` breaks the rule.
            pub fn try_new(id: impl Into<String>) -> Result<Self, IdError> {
                let id = id.into();
                validate(&id)?;
                Ok(Self(id))
            }

            /// The id as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdError;
            fn try_from(id: &str) -> Result<Self, IdError> {
                Self::try_new(id)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(id: String) -> Result<Self, IdError> {
                Self::try_new(id)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

slug_id! {
    /// A step's identity: chosen by the caller, unique within a plan, short enough for an agent to read and copy.
    /// Separate from the step's free-form display name.
    StepId
}

slug_id! {
    /// One run's identity, supplied by the caller. Resume reuses it, so a step's idempotency key
    /// (`run_id:step_id`) stays the same across attempts and across resume. Reusing a run id for a different plan or
    /// different inputs makes downstream deduplication treat new work as a repeat; michi cannot detect that.
    RunId
}

impl RunId {
    /// Builds a run id that is unique in practice without a UUID dependency:
    /// `{prefix}-{millis}-{pid}-{counter}-{random}`, the numbers in base 36. `random` is 64 bits from the standard
    /// library's per-process randomly keyed hasher, mixed with the other parts, so two hosts that start in the same
    /// millisecond with the same pid and counter (containers all run as pid 1) still get different ids. The run id is
    /// the prefix of every step's idempotency key, so a collision would let a downstream that deduplicates treat new
    /// work as a repeat of old.
    ///
    /// This is unique with overwhelming probability, not by construction: if you need a global guarantee (a
    /// deduplicating downstream that outlives you), supply your own UUID or other id from a source you trust with
    /// [`RunId::try_new`]. The random part takes 14 of the [`MAX_ID_LEN`] bytes, so keep the prefix short (about 30
    /// bytes at most).
    ///
    /// # Errors
    ///
    /// An [`IdError`] if `prefix` is not a valid slug or the result would exceed [`MAX_ID_LEN`].
    pub fn unique(prefix: &str) -> Result<Self, IdError> {
        use std::hash::BuildHasher;

        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        validate(prefix)?;
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let pid = u64::from(std::process::id());
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // `RandomState` is keyed from the OS's randomness once per process (and stepped per instance), so the hash is
        // unpredictable across processes even when millis, pid and seq match.
        let entropy = std::collections::hash_map::RandomState::new().hash_one((millis, pid, seq));
        Self::try_new(unique_id(prefix, millis, pid, seq, entropy))
    }
}

/// `{prefix}-{millis}-{pid}-{seq}-{entropy}` in base 36. Every number has its own separator: base-36 numbers vary in
/// length, so without one two different (pid, seq) pairs could print the same digits.
fn unique_id(prefix: &str, millis: u64, pid: u64, seq: u64, entropy: u64) -> String {
    let mut id = String::with_capacity(prefix.len() + 4 + 4 * 13);
    id.push_str(prefix);
    for n in [millis, pid, seq, entropy] {
        id.push('-');
        push_base36(&mut id, n);
    }
    id
}

/// Appends `n` in lowercase base 36.
fn push_base36(out: &mut String, mut n: u64) {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = [0u8; 13];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = DIGITS[usize::try_from(n % 36).unwrap_or(0)];
        n /= 36;
        if n == 0 {
            break;
        }
    }
    for &b in &buf[i..] {
        out.push(char::from(b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_readable_slugs() {
        for ok in ["fetch", "build_2", "deploy-prod", "v1.2", "_internal", "9lives"] {
            assert_eq!(StepId::try_new(ok).map(|id| id.to_string()), Ok(ok.to_string()));
        }
    }

    #[test]
    fn rejects_empty_long_and_out_of_charset_ids() {
        assert_eq!(StepId::try_new(""), Err(IdError::Empty));
        assert_eq!(StepId::try_new("a".repeat(65)), Err(IdError::TooLong { len: 65, max: 64 }));
        assert!(StepId::try_new("a".repeat(64)).is_ok());
        assert_eq!(StepId::try_new("run:step"), Err(IdError::InvalidChar { ch: ':', index: 3 }));
        assert_eq!(StepId::try_new("-lead"), Err(IdError::InvalidChar { ch: '-', index: 0 }));
        assert_eq!(StepId::try_new("has space"), Err(IdError::InvalidChar { ch: ' ', index: 3 }));
        assert_eq!(RunId::try_new("é"), Err(IdError::InvalidChar { ch: 'é', index: 0 }));
    }

    #[test]
    fn unique_run_ids_differ_and_stay_valid_slugs() {
        let a = RunId::unique("deploy").unwrap();
        let b = RunId::unique("deploy").unwrap();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("deploy-"));
        assert!(a.as_str().len() <= MAX_ID_LEN);
        assert!(RunId::unique("bad:prefix").is_err());
        assert!(RunId::unique(&"p".repeat(60)).is_err(), "the suffix pushes it past the limit");
    }

    #[test]
    fn unique_ids_separate_the_pid_from_the_counter() {
        // Without a separator, pid 1388 ("12k") + counter 1152 ("w0") and pid 50000 ("12kw") + counter 0 ("0") both
        // printed "12kw0".
        let a = unique_id("run", 1, 1388, 1152, 7);
        let b = unique_id("run", 1, 50_000, 0, 7);
        assert_ne!(a, b);
        assert_eq!(a, "run-1-12k-w0-7");
        assert_eq!(b, "run-1-12kw-0-7");
        let id = RunId::unique("deploy").unwrap();
        assert_eq!(id.as_str().split('-').count(), 5, "{id}");
        let widest = unique_id("p", u64::MAX, u64::from(u32::MAX), u64::MAX, u64::MAX);
        assert!(RunId::try_new(widest).is_ok(), "fits the limit");
    }

    #[test]
    fn base36_encodes_zero_and_max() {
        let mut s = String::new();
        push_base36(&mut s, 0);
        push_base36(&mut s, 35);
        assert_eq!(s, "0z");
        let mut max = String::new();
        push_base36(&mut max, u64::MAX);
        assert_eq!(max, "3w5e11264sgsf");
    }
}
