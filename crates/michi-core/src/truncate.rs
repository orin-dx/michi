#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # Truncation
//!
//! Cut one string on a UTF-8 char boundary and report the cut out of band.
//!
//! [`truncate()`] returns the kept text and the number of chars removed in separate fields, so the text never carries
//! a message that a value could imitate. This is the same rule a cut TOON cell follows: keep `max_chars - 1` chars
//! and end in `…`. For what a cap on a **list** cut, see [`crate::truncation`].

use crate::hints::Hint;

/// The marker a cut ends with (U+2026 HORIZONTAL ELLIPSIS).
const ELLIPSIS: char = '\u{2026}';

/// Result of cutting one string with [`truncate()`].
///
/// `#[non_exhaustive]`: read the fields, but build values only through [`truncate()`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Truncated {
    /// The text to show: the input unchanged when it fits, otherwise its first `max_chars - 1` chars followed by a
    /// plain `…` (so exactly `max_chars` chars). A cap of 0 gives the empty string.
    pub content: String,
    /// Length of the input in chars.
    pub original_chars: usize,
    /// How many chars of the input are not in `content`: `original_chars` minus the kept chars, not counting the
    /// `…`. `0` when nothing was cut.
    pub removed: usize,
}

impl Truncated {
    /// Whether any chars were cut.
    #[must_use]
    pub fn was_truncated(&self) -> bool {
        self.removed > 0
    }

    /// The `help[]` hint reporting the cut: `cut {removed} of {original_chars} chars; {guidance}`, or
    /// `cut {removed} of {original_chars} chars` when `guidance` is empty. `None` when nothing was cut.
    ///
    /// `guidance` is the caller's instruction for getting the rest (`pass full=true`): michi never invents a flag
    /// the calling tool may not have. Add the hint to the response's trusted `help[]` hints, not to the text itself,
    /// so the report cannot be forged by the data.
    #[must_use]
    pub fn hint(&self, guidance: impl Into<String>) -> Option<Hint> {
        if !self.was_truncated() {
            return None;
        }
        let guidance = guidance.into();
        let removed = self.removed.to_string();
        let original = self.original_chars.to_string();
        let mut text = String::with_capacity(16 + removed.len() + original.len() + guidance.len());
        text.push_str("cut ");
        text.push_str(&removed);
        text.push_str(" of ");
        text.push_str(&original);
        text.push_str(" chars");
        if !guidance.is_empty() {
            text.push_str("; ");
            text.push_str(&guidance);
        }
        Some(Hint::new(text))
    }
}

/// Cut `content` to at most `max_chars` Unicode scalar values.
///
/// Text that fits is returned unchanged. A longer text keeps its first `max_chars - 1` chars and ends in `…`, so the
/// result is exactly `max_chars` chars; a cap of 1 gives `…` and a cap of 0 gives the empty string. How much was cut
/// is in [`Truncated::removed`] and [`Truncated::hint`], never in `content`.
///
/// Uses char boundaries: never splits a Unicode scalar, though it can split a multi-part emoji or flag pair, which
/// stays valid UTF-8.
#[must_use]
pub fn truncate(content: &str, max_chars: usize) -> Truncated {
    let chars = content.chars().count();
    if chars <= max_chars {
        return Truncated { content: content.to_string(), original_chars: chars, removed: 0 };
    }

    let kept = max_chars.saturating_sub(1);
    let byte_end = content.char_indices().nth(kept).map_or(content.len(), |(i, _)| i);
    let mut out = String::with_capacity(byte_end + ELLIPSIS.len_utf8());
    out.push_str(&content[..byte_end]);
    if max_chars > 0 {
        out.push(ELLIPSIS);
    }
    Truncated { content: out, original_chars: chars, removed: chars - kept }
}
