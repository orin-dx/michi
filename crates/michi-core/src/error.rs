use std::time::Duration;

use crate::hints::Hint;
use crate::recovery::RecoveryHint;
use crate::response::OutputFormat;

/// Classification of a [`DomainError`] for routing and display decisions; see [`DomainError::class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorClass {
    /// The caller provided invalid input. Do not retry.
    User,
    /// A downstream or infrastructure failure that is not the caller's direct
    /// error and is not expected to self-resolve without intervention. Note:
    /// `RateLimited` falls here when `retryable: false` — the caller may have
    /// contributed to the rate-limit condition, but at classification time the
    /// error is not self-resolving. michi does not recommend automatic retry
    /// for `Internal` errors.
    Internal,
    /// A failure that trying the same call again is expected to fix, without repeating an effect: the request was
    /// not processed (rate limited, unavailable), or the caller marked the error retryable because the operation is
    /// idempotent.
    Transient,
    /// A timeout: no answer came back, so the operation may or may not have taken effect. Not the caller's fault and
    /// not an internal error: verify whether it completed before retrying, unless the operation is idempotent, in
    /// which case the caller marks it retryable and it is [`ErrorClass::Transient`].
    Ambiguous,
}

/// The hint a fresh `Timeout` [`DomainError`] carries until it is marked retryable.
pub const MAY_HAVE_TAKEN_EFFECT_HINT: &str =
    "the operation may have taken effect; verify whether it completed before retrying, unless it is idempotent";

/// Wraps a value so its inner content is omitted from `Debug` and `Display`.
///
/// That is all it does: the field is public, and none of michi's renderers takes a `Sensitive`. Text passed to a hint,
/// an error message or a cause is rendered as given, so keep secrets out of those strings in the first place.
#[derive(Clone)]
pub struct Sensitive<T>(pub T);

impl<T> std::fmt::Debug for Sensitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl<T> std::fmt::Display for Sensitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The specific kind of domain error.
///
/// Rendered output uses the snake_case [`ErrorCode::label`], which is also its `Display` and its `FromStr` input; with
/// the `schemars` feature its schema is the enum of those labels, matching the `error` field of the JSON form of
/// [`DomainError::render`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum ErrorCode {
    /// Bad parameters.
    InvalidInput,
    /// Resource absent.
    NotFound,
    /// Auth failure.
    Unauthorized,
    /// Permission denied.
    Forbidden,
    /// Resource state mismatch.
    Conflict,
    /// Rate limited (HTTP 429).
    RateLimited,
    /// Service unavailable (HTTP 503).
    Unavailable,
    /// Request timed out.
    Timeout,
    /// Downstream/external failure.
    ExternalFailure,
}

impl ErrorCode {
    /// The snake_case label used in rendered output.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::ExternalFailure => "external_failure",
        }
    }

    /// Every code, in declaration order. `FromStr` searches this list, so a label is parsed by the same table that
    /// [`label`](Self::label) writes; a unit test names every variant without a wildcard, so a new one fails to
    /// compile there until it is listed here.
    pub const ALL: [Self; 9] = [
        Self::InvalidInput,
        Self::NotFound,
        Self::Unauthorized,
        Self::Forbidden,
        Self::Conflict,
        Self::RateLimited,
        Self::Unavailable,
        Self::Timeout,
        Self::ExternalFailure,
    ];

    /// Whether a fresh [`DomainError`] with this code is retryable: `RateLimited` and `Unavailable` only, which mean
    /// the request was not processed. `Timeout` and `ExternalFailure` are not: the request may have taken effect
    /// (RFC 9110 §15.6), so retrying is safe only for an idempotent operation, and the caller says so with
    /// [`DomainError::retryable`].
    #[must_use]
    pub fn is_retryable_by_default(&self) -> bool {
        matches!(self, Self::RateLimited | Self::Unavailable)
    }

    /// Whether a failure with this code leaves it unknown whether the request was applied: `Timeout`, where no
    /// answer came back. `ExternalFailure` is too broad to say (a failed local walk, a server error), so the caller
    /// adds a hint when it knows; `RateLimited` and `Unavailable` mean the request was not processed.
    #[must_use]
    pub fn may_have_taken_effect(&self) -> bool {
        matches!(self, Self::Timeout)
    }

    /// The default [`ErrorClass`] for this code, independent of whether an error is marked retryable.
    ///
    /// `Timeout` returns `Ambiguous` (the operation may have taken effect); `RateLimited`, `Unavailable` and
    /// `ExternalFailure` return `Internal`; caller-fault codes return `User`. Used by [`DomainError::class`] when the
    /// error is not marked retryable.
    #[must_use]
    pub fn default_class(&self) -> ErrorClass {
        match self {
            Self::InvalidInput | Self::NotFound | Self::Unauthorized | Self::Forbidden | Self::Conflict => {
                ErrorClass::User
            }
            Self::RateLimited | Self::Unavailable | Self::ExternalFailure => ErrorClass::Internal,
            Self::Timeout => ErrorClass::Ambiguous,
        }
    }
}

/// The [`label`](ErrorCode::label).
impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A string that is not an [`ErrorCode`] label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownErrorCode(pub String);

impl std::fmt::Display for UnknownErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown error code {:?}; expected one of: ", self.0)?;
        for (i, code) in ErrorCode::ALL.iter().enumerate() {
            f.write_str(if i == 0 { "" } else { ", " })?;
            f.write_str(code.label())?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownErrorCode {}

impl std::str::FromStr for ErrorCode {
    type Err = UnknownErrorCode;

    /// The code whose [`label`](ErrorCode::label) is `s`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|code| code.label() == s).ok_or_else(|| UnknownErrorCode(s.to_string()))
    }
}

/// A domain-level error.
///
/// `causes` say why it happened (diagnosis); `hints` say what to do next (guidance). They are separate fields, and
/// render as separate keys, so an agent can tell them apart. [`DomainError::from_error_with_causes`] fills `message`
/// and `causes` from a `std::error::Error` and its `source()` chain.
///
/// The fields are private: build the error with [`DomainError::new`] and the builder methods, and read it with the
/// getters. The builder keeps [`MAY_HAVE_TAKEN_EFFECT_HINT`] in step with the code and the retry flag, which a field
/// write would skip (a `Timeout` set retryable after construction kept the "verify first" hint; a `RateLimited` error
/// whose code became `Timeout` was retryable with no hint at all).
///
/// ```compile_fail,E0616
/// let mut e = michi_core::DomainError::new(michi_core::ErrorCode::Timeout, "POST /charge timed out");
/// e.retryable = true;
/// ```
///
/// ```compile_fail,E0616
/// let mut e = michi_core::DomainError::new(michi_core::ErrorCode::RateLimited, "slow down");
/// e.code = michi_core::ErrorCode::Timeout;
/// ```
///
/// With the `serde` feature it serializes to exactly the JSON of `render(OutputFormat::Json)`
/// (`{isError, error, message, causes, retryable, hints, recovery}`), and with `schemars` its schema describes that
/// form, so it can be used as an MCP `outputSchema`. `retry_after` is not part of that form.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(into = "DomainErrorWire"))]
#[non_exhaustive]
pub struct DomainError {
    code: ErrorCode,
    message: String,
    causes: Vec<String>,
    hints: Vec<Hint>,
    recovery: Option<RecoveryHint>,
    retryable: bool,
    retry_after: Option<Duration>,
}

impl DomainError {
    /// Create a domain error. The message is tool-author text, never data: michi renders it as the error's own
    /// voice, and a value copied from a file, web page or user field can carry instructions. See the trust model in
    /// `docs/spec/02-toon-format.md`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let mut err = Self {
            retryable: code.is_retryable_by_default(),
            code,
            message: message.into(),
            causes: Vec::new(),
            hints: Vec::new(),
            recovery: None,
            retry_after: None,
        };
        err.sync_effect_hint();
        err
    }

    /// Keeps [`MAY_HAVE_TAKEN_EFFECT_HINT`] present exactly while the error's code may have taken effect and it is not
    /// marked retryable.
    fn sync_effect_hint(&mut self) {
        let wanted = self.code.may_have_taken_effect() && !self.retryable;
        let present = self.hints.iter().any(|h| h.as_str() == MAY_HAVE_TAKEN_EFFECT_HINT);
        if wanted && !present {
            self.hints.push(Hint::new(MAY_HAVE_TAKEN_EFFECT_HINT));
        } else if !wanted && present {
            self.hints.retain(|h| h.as_str() != MAY_HAVE_TAKEN_EFFECT_HINT);
        }
    }

    /// Build a domain error from an existing error, keeping only its own `Display` as `message`.
    ///
    /// The `source()` chain is **not** copied: underlying errors often carry URLs with tokens, file paths and other
    /// internals that should not reach an agent. Use [`DomainError::from_error_with_causes`] to opt in to the chain.
    /// The top-level `Display` is still agent-visible, so make sure it is safe to show; if it is not, build the
    /// error with [`DomainError::new`] and a message you wrote. The message is tool-author text, never data. As a
    /// safety net the text goes through [`crate::redact::redact_common_shapes`]: URL userinfo, the values of
    /// `token=`/`api_key=`/`password=`-style pairs (a `reqwest` error puts the whole URL, query included, in its
    /// top-level `Display`) and a few token prefixes become `[REDACTED]`. That is best effort, not a guarantee: see
    /// that function for what it misses.
    ///
    /// ```
    /// use michi_core::{DomainError, ErrorCode};
    ///
    /// #[derive(Debug, thiserror::Error)]
    /// #[error("sync failed")]
    /// struct Sync(#[source] std::io::Error);
    ///
    /// let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "/home/ci/.netrc: permission denied");
    /// let e = DomainError::from_error(ErrorCode::ExternalFailure, &Sync(io));
    /// assert_eq!(e.message(), "sync failed");
    /// assert!(e.causes().is_empty());
    /// ```
    pub fn from_error(code: ErrorCode, err: &(dyn std::error::Error + 'static)) -> Self {
        Self::new(code, crate::redact::redact_common_shapes(&err.to_string()))
    }

    /// Like [`DomainError::from_error`], and also copies the `source()` chain into `causes`. Opt-in: every link's
    /// `Display` becomes agent-visible text, so use it only when the whole chain is safe to show (no URLs with
    /// tokens, no paths or internals you would not print to an end user). The same redaction is applied to every link, as in
    /// [`DomainError::from_error`].
    ///
    /// Each `source()` link, outermost first, adds its `Display` as one entry of `causes` (never a hint: causes are
    /// diagnosis, hints are guidance). A link whose text is already contained in the last text kept (the message, or
    /// the previous cause kept) is skipped, because `thiserror` messages often embed their source
    /// (`#[error("walk failed: {0}")]`) and would otherwise repeat it. Line breaks stay inside their entry; the
    /// renderers escape them. The walk stops after 128 links, so a cyclic `source()` cannot loop.
    ///
    /// The chain is kept as text only: no source error is stored, so [`std::error::Error::source`] on the result is
    /// still `None`, and the result stays `Clone`, `Send` and `Sync`. Add a prefix with
    /// [`context`](Self::context).
    ///
    /// ```
    /// use michi_core::{DomainError, ErrorCode};
    ///
    /// #[derive(Debug, thiserror::Error)]
    /// #[error("walk failed: {0}")]
    /// struct Walk(#[source] std::io::Error);
    ///
    /// let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
    /// let e = DomainError::from_error_with_causes(ErrorCode::ExternalFailure, &Walk(io));
    /// assert_eq!(e.message(), "walk failed: permission denied");
    /// assert!(e.causes().is_empty(), "the message already holds the only cause");
    /// ```
    pub fn from_error_with_causes(code: ErrorCode, err: &(dyn std::error::Error + 'static)) -> Self {
        let mut out = Self::from_error(code, err);
        let mut link = err.source();
        for _ in 0..MAX_CAUSE_LINKS {
            let Some(e) = link else { break };
            let text = crate::redact::redact_common_shapes(&e.to_string());
            let previous = out.causes.last().unwrap_or(&out.message);
            if !previous.contains(text.as_str()) {
                out.causes.push(text);
            }
            link = e.source();
        }
        out
    }

    /// Append one cause (why the error happened), rendered after the message as `causes[N]: …`. Tool-author text,
    /// never data, and nothing you would not show an end user (no tokens, internal paths).
    #[must_use]
    pub fn cause(mut self, cause: impl Into<String>) -> Self {
        self.causes.push(cause.into());
        self
    }

    /// Append a contextual hint. Tool-author text, never data.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint::new(hint));
        self
    }

    /// Attach a structured recovery hint.
    #[must_use]
    pub fn recovery(mut self, r: RecoveryHint) -> Self {
        self.recovery = Some(r);
        self
    }

    /// Override the default retryability for this error instance. Pass `true` for a `Timeout` or `ExternalFailure`
    /// only when repeating the operation is safe (it is idempotent, or known not to have taken effect).
    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self.sync_effect_hint();
        self
    }

    /// Attach a `Retry-After` delay, or clear it with `None`.
    #[must_use]
    pub fn retry_after(mut self, d: impl Into<Option<Duration>>) -> Self {
        self.retry_after = d.into();
        self
    }

    /// Prefix the message with `context`, as `"{context}: {message}"`, keeping everything else. Tool-author text, never
    /// data.
    ///
    /// ```
    /// use michi_core::{DomainError, ErrorCode};
    ///
    /// let e = DomainError::new(ErrorCode::NotFound, "no such file").context("load config");
    /// assert_eq!(e.message(), "load config: no such file");
    /// ```
    #[must_use]
    pub fn context(mut self, context: impl std::fmt::Display) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }

    /// The error's code.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Why it happened, outermost first: one entry per underlying cause, rendered as `causes[N]: …` after the message.
    #[must_use]
    pub fn causes(&self) -> &[String] {
        &self.causes
    }

    /// The contextual hints, rendered as `help[N]: …`, including [`MAY_HAVE_TAKEN_EFFECT_HINT`] while it applies.
    #[must_use]
    pub fn hints(&self) -> &[Hint] {
        &self.hints
    }

    /// The structured recovery hint, if one was attached with [`recovery`](Self::recovery).
    #[must_use]
    pub fn get_recovery(&self) -> Option<&RecoveryHint> {
        self.recovery.as_ref()
    }

    /// The `Retry-After` delay attached with [`retry_after`](Self::retry_after), if any.
    #[must_use]
    pub fn get_retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    /// Whether calling again is expected to help without repeating an effect: by default `true` only for
    /// `RateLimited` and `Unavailable` (the request was not processed), or after [`retryable(true)`](Self::retryable)
    /// for an idempotent operation. True exactly when [`class`](Self::class) is [`ErrorClass::Transient`].
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    /// Classify this error for routing: [`ErrorClass::Transient`] when it is retryable, otherwise the code's
    /// [`default_class`](ErrorCode::default_class).
    #[must_use]
    pub fn class(&self) -> ErrorClass {
        if self.retryable {
            ErrorClass::Transient
        } else {
            self.code.default_class()
        }
    }

    /// The process exit code to use when this error is fatal. Always `1`.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        1
    }

    /// Render in the requested format.
    ///
    /// [`OutputFormat::Text`] is a TOON 4.4 root object, with no trailing newline: `error: <code label>`, `message: …`
    /// (quoted per §7.2), the inline `causes[N]: …` array when there are causes, `exit_code: 1`, then the inline
    /// `help[N]: …` array and the `recovery[1]{tool,params,reason}:` table when present.
    ///
    /// [`OutputFormat::Json`] is a compact JSON object, the `structured_content` companion of the text. Keys:
    /// `isError`, `error`, `message`, `causes`, `retryable`, `hints`, `recovery`. `causes` and `hints` are always
    /// present, as `[]` when empty; `recovery` is the hint object, or `null`. The shape differs from
    /// `AgentResponse`'s JSON (`{body, hints, recovery, isError}`, with `recovery` an array); only `isError` and
    /// `hints` mean the same in both.
    #[must_use]
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Text => self.render_text(),
            OutputFormat::Json => self.render_json(),
        }
    }

    fn render_text(&self) -> String {
        let causes_est: usize = self.causes.iter().map(|c| c.len() + 3).sum();
        let mut out = String::with_capacity(96 + self.message.len() + causes_est + self.hints.len() * 50);
        push_error_fields(&mut out, self.code.label(), &self.message, &self.causes);
        crate::hints::append_hints(&mut out, &self.hints);
        if let Some(r) = &self.recovery {
            crate::recovery::append_recovery(&mut out, std::slice::from_ref(r));
        }
        out
    }

    /// Render formatted for GitHub Actions workflow annotations (`::error title=code::message`).
    ///
    /// The message is encoded as GitHub workflow command data: `%` becomes `%25` first, then CR and LF become `%0D`
    /// and `%0A`, so the annotation stays on one line and a literal `%0A` in the message cannot decode to a line
    /// break. The `title` property is encoded the same way plus `:` → `%3A` and `,` → `%2C`.
    #[must_use]
    pub fn render_github_annotation(&self) -> String {
        format!(
            "::error title={}::{}",
            escape_annotation_property(self.code.label()),
            escape_annotation_data(&self.message)
        )
    }

    fn render_json(&self) -> String {
        let hints_est: usize = self.hints.iter().map(|h| h.as_str().len() + 4).sum();
        let causes_est: usize = self.causes.iter().map(|c| c.len() + 4).sum();
        let recovery_est: usize = self.recovery.as_ref().map_or(0, |r| {
            50 + r.tool.len()
                + r.params.iter().map(|_| 20usize).sum::<usize>()
                + r.reason.as_ref().map_or(0, |s| s.len() + 12)
        });
        let mut out = String::with_capacity(140 + self.message.len() + causes_est + hints_est + recovery_est);
        out.push_str("{\"isError\":true,\"error\":");
        crate::json::push_str(&mut out, self.code.label());
        out.push_str(",\"message\":");
        crate::json::push_str(&mut out, &self.message);
        out.push_str(",\"causes\":");
        crate::json::push_str_array(&mut out, self.causes.iter().map(String::as_str));
        out.push_str(",\"retryable\":");
        crate::json::push_bool(&mut out, self.retryable);
        out.push_str(",\"hints\":");
        crate::json::push_str_array(&mut out, self.hints.iter().map(Hint::as_str));
        out.push_str(",\"recovery\":");
        match &self.recovery {
            None => out.push_str("null"),
            Some(r) => crate::json::push_recovery(&mut out, r),
        }
        out.push('}');
        out
    }

    /// Build an MCP [`CallToolResult`](crate::mcp::CallToolResult) representing this error.
    ///
    /// Sets `is_error: true`. Text content is `render(OutputFormat::Text)`; `structured_content` is
    /// `render(OutputFormat::Json)`.
    ///
    /// **Canonical path:** call this directly when you hold a `DomainError`.
    /// Do not wrap in `AgentResponse::as_error()` — that loses typed fields.
    #[must_use]
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        crate::mcp::CallToolResult::new(
            vec![crate::mcp::ContentBlock::new(self.render_text(), vec![crate::audience::Audience::Assistant])],
            true,
            Some(crate::mcp::JsonObject::from_trusted(self.render_json())),
        )
    }
}

/// The wire form of [`DomainError`]: the object `render(OutputFormat::Json)` writes.
#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "DomainError"))]
#[serde(rename_all = "camelCase")]
struct DomainErrorWire {
    /// Always `true`.
    #[cfg_attr(feature = "schemars", schemars(schema_with = "const_true_schema"))]
    is_error: bool,
    /// The snake_case error code label.
    #[cfg_attr(feature = "schemars", schemars(with = "ErrorCode"))]
    error: &'static str,
    /// Human-readable message.
    message: String,
    /// Why it happened, outermost first; `[]` when unknown.
    causes: Vec<String>,
    /// Whether calling again is expected to help without repeating an effect.
    retryable: bool,
    /// Contextual hints.
    hints: Vec<String>,
    /// The structured recovery hint, or `null`.
    recovery: Option<RecoveryHint>,
}

#[cfg(feature = "schemars")]
fn const_true_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "type": "boolean", "const": true })
}

#[cfg(feature = "serde")]
impl From<DomainError> for DomainErrorWire {
    fn from(e: DomainError) -> Self {
        Self {
            is_error: true,
            error: e.code.label(),
            message: e.message,
            causes: e.causes,
            retryable: e.retryable,
            hints: e.hints.iter().map(|h| h.as_str().to_string()).collect(),
            recovery: e.recovery,
        }
    }
}

// The schema describes the JSON render/serde wire form above, not the Rust field layout.
#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl schemars::JsonSchema for DomainError {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        DomainErrorWire::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        DomainErrorWire::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        DomainErrorWire::json_schema(generator)
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.label(), self.message)
    }
}

/// `source()` is always `None`: [`DomainError::from_error_with_causes`] keeps a chain as text in `causes`, not as a stored error.
impl std::error::Error for DomainError {}

/// The most `source()` links [`DomainError::from_error_with_causes`] walks, so a cyclic chain cannot loop forever.
const MAX_CAUSE_LINKS: usize = 128;

/// Encode GitHub workflow command data (the message): `%` first, so an encoded sequence in the input stays literal,
/// then CR and LF.
fn escape_annotation_data(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            other => out.push(other),
        }
    }
    out
}

/// Encode a GitHub workflow command property value: the data encoding plus `:` and `,`, which delimit properties.
fn escape_annotation_property(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            ':' => out.push_str("%3A"),
            ',' => out.push_str("%2C"),
            other => out.push(other),
        }
    }
    out
}

/// The fields every rendered error document starts with: `error: <code>`, `message: …`, `causes[N]: …` (only when
/// there are causes), `exit_code: 1`.
fn push_error_fields(out: &mut String, code: &str, message: &str, causes: &[String]) {
    out.push_str("error: ");
    out.push_str(code);
    out.push_str("\nmessage: ");
    out.push_str(&crate::toon::escape_value(message));
    crate::hints::append_inline_strings(out, "causes", causes.iter().map(String::as_str));
    out.push_str("\nexit_code: 1");
}

#[cfg(feature = "miette")]
#[cfg_attr(docsrs, doc(cfg(feature = "miette")))]
impl miette::Diagnostic for DomainError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.code.label()))
    }

    /// The hints, one per line, then one `caused by: …` line per cause; `None` when there are neither.
    ///
    /// miette prints a `source()` chain itself, but [`DomainError`] stores its causes as text, so they go here,
    /// labelled and after the guidance.
    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        if self.hints.is_empty() && self.causes.is_empty() {
            return None;
        }
        let lines: Vec<String> = self
            .hints
            .iter()
            .map(|h| h.as_str().to_string())
            .chain(self.causes.iter().map(|c| format!("caused by: {c}")))
            .collect();
        Some(Box::new(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_error_render_json_basic() {
        let err = DomainError::new(ErrorCode::NotFound, "Item 42 not found");
        let json = err.render(OutputFormat::Json);
        assert!(json.contains("\"isError\":true"), "got: {json}");
        assert!(json.contains("\"error\":"), "got: {json}");
        assert!(json.contains("\"message\":\"Item 42 not found\""), "got: {json}");
        assert!(json.contains("\"retryable\":false"), "got: {json}");
        assert!(json.contains("\"hints\":["), "got: {json}");
    }

    #[test]
    fn domain_error_render_json_includes_hints() {
        let err = DomainError::new(ErrorCode::NotFound, "not found").hint("Try searching with get_items");
        let json = err.render(OutputFormat::Json);
        assert!(json.contains("\"hints\""), "got: {json}");
        assert!(json.contains("Try searching"), "got: {json}");
    }

    #[test]
    fn domain_error_to_call_tool_result_is_error() {
        let err = DomainError::new(ErrorCode::Unavailable, "down");
        let result = err.to_call_tool_result();
        assert!(result.is_error);
        assert!(!result.content.is_empty());
    }

    #[test]
    fn domain_error_json_escapes_message() {
        let err = DomainError::new(ErrorCode::InvalidInput, r#"field "name" invalid"#);
        let json = err.render(OutputFormat::Json);
        assert!(json.contains("\\\"name\\\""), "quotes in message must be JSON-escaped, got: {json}");
    }

    #[test]
    fn domain_error_renders_kv_block_with_hints() {
        let e = DomainError::new(ErrorCode::NotFound, "Issue #9999 does not exist in this repository")
            .hint("Call list_issues to see available numbers");
        let out = e.render(OutputFormat::Text);
        assert_eq!(
            out,
            "error: not_found\nmessage: Issue #9999 does not exist in this repository\nexit_code: 1\nhelp[1]: Call list_issues to see available numbers"
        );
    }

    #[test]
    fn domain_error_renders_github_annotation() {
        let e = DomainError::new(ErrorCode::InvalidInput, "field 'name' is required");
        assert_eq!(e.render_github_annotation(), "::error title=invalid_input::field 'name' is required");
    }

    #[test]
    fn github_annotation_encodes_newlines_and_cr() {
        let e = DomainError::new(ErrorCode::InvalidInput, "line1\nline2\r\nline3");
        let out = e.render_github_annotation();
        assert!(!out.contains('\n'), "annotation must not contain literal newline");
        assert!(!out.contains('\r'), "annotation must not contain literal CR");
        assert!(out.contains("%0A"), "LF must be encoded as %0A");
        assert!(out.contains("%0D"), "CR must be encoded as %0D");
    }

    // AC-017: the mixed-character test above only proves %0A and %0D are both
    // present, not which source character maps to which -- a swapped mapping
    // would pass it too. Isolate each character singly (where the other
    // encoding cannot legally appear) to pin the exact direction.
    #[test]
    fn ac017_lf_maps_to_pct0a_and_cr_maps_to_pct0d_specifically() {
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "a\nb").render_github_annotation(),
            "::error title=invalid_input::a%0Ab"
        );
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "a\rb").render_github_annotation(),
            "::error title=invalid_input::a%0Db"
        );
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "line1\nline2\r\nline3").render_github_annotation(),
            "::error title=invalid_input::line1%0Aline2%0D%0Aline3"
        );
    }

    // GitHub decodes `%25`, `%0D` and `%0A` in workflow command data, so a literal `%` must be encoded first or a
    // message containing `%0A` would decode to a forged line break.
    #[test]
    fn github_annotation_encodes_percent_before_line_breaks() {
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "50%0Aforged").render_github_annotation(),
            "::error title=invalid_input::50%250Aforged"
        );
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "100%25").render_github_annotation(),
            "::error title=invalid_input::100%2525"
        );
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "a%\nb").render_github_annotation(),
            "::error title=invalid_input::a%25%0Ab"
        );
    }

    // Property values additionally encode `:` and `,`, which delimit properties.
    #[test]
    fn github_annotation_property_escaping_covers_colon_and_comma() {
        assert_eq!(escape_annotation_property("a:b,c%d\ne\rf"), "a%3Ab%2Cc%25d%0Ae%0Df");
        // The message (data) keeps `:` and `,` verbatim.
        assert_eq!(
            DomainError::new(ErrorCode::InvalidInput, "a:b,c").render_github_annotation(),
            "::error title=invalid_input::a:b,c"
        );
    }

    // AC-016: every prior exact-literal witness for render_github_annotation()
    // uses a DomainError with no hints and no recovery, leaving the universal
    // "exactly {label}::{message}" claim unproven against those fields --
    // unlike AC-012a, which does prove retry_after contributes nothing here.
    #[test]
    fn ac016_render_github_annotation_ignores_hints_and_recovery() {
        let e = DomainError::new(ErrorCode::InvalidInput, "m").hint("h").recovery(RecoveryHint::new("t"));
        assert_eq!(e.render_github_annotation(), "::error title=invalid_input::m");
    }

    #[test]
    fn render_json_output_is_valid_json_with_all_fields() {
        let err = DomainError::new(ErrorCode::NotFound, r#"has "quotes" and \backslash"#)
            .hint("call list_items")
            .hint("check the id")
            .retryable(false);
        let json = err.render(OutputFormat::Json);
        assert!(json.starts_with('{') && json.ends_with('}'), "must be a JSON object, got: {json}");
        assert!(json.contains("\"isError\":true"), "isError must be true, got: {json}");
        assert!(json.contains("\"hints\":["), "must include hints array, got: {json}");
        assert!(json.contains("\"recovery\":null"), "must include recovery null, got: {json}");
        // Escaped quotes inside string values must use \" not raw "
        assert!(!json.contains(r#"has "quotes""#), "inner quotes must be escaped, got: {json}");
    }

    #[test]
    fn render_json_capacity_covers_hints_and_recovery() {
        let err = DomainError::new(ErrorCode::Unavailable, "service down")
            .hint("wait and retry")
            .hint("check status page")
            .hint("contact support if persists")
            .retryable(true)
            .recovery(
                crate::recovery::RecoveryHint::new("retry_request")
                    .param("after_ms", crate::kv::KvValue::Int(5000))
                    .reason("Rate limit window resets every 60 seconds"),
            );
        let json = err.render(OutputFormat::Json);
        // Smoke test: no panic = capacity was sufficient (Vec didn't reallocate unexpectedly).
        // JSON correctness is validated by structure checks.
        assert!(json.contains("\"hints\":["), "got: {json}");
        assert!(json.contains("\"recovery\":{"), "got: {json}");
        assert!(json.contains("\"reason\":"), "got: {json}");
    }

    #[test]
    fn unavailable_with_retryable_false_is_internal_not_user() {
        let err = DomainError::new(ErrorCode::Unavailable, "down").retryable(false);
        assert_eq!(err.class(), ErrorClass::Internal, "infra error with retryable=false must be Internal, not User");
    }

    #[test]
    fn invalid_input_is_user() {
        let err = DomainError::new(ErrorCode::InvalidInput, "bad input");
        assert_eq!(err.class(), ErrorClass::User);
    }

    #[test]
    fn rate_limited_retryable_true_is_transient() {
        // Default: RateLimited is retryable by default
        let err = DomainError::new(ErrorCode::RateLimited, "429");
        assert_eq!(err.class(), ErrorClass::Transient);
    }

    #[test]
    fn default_class_for_all_codes() {
        use ErrorCode::*;
        assert_eq!(InvalidInput.default_class(), ErrorClass::User);
        assert_eq!(NotFound.default_class(), ErrorClass::User);
        assert_eq!(Unauthorized.default_class(), ErrorClass::User);
        assert_eq!(Forbidden.default_class(), ErrorClass::User);
        assert_eq!(Conflict.default_class(), ErrorClass::User);
        assert_eq!(RateLimited.default_class(), ErrorClass::Internal);
        assert_eq!(Unavailable.default_class(), ErrorClass::Internal);
        assert_eq!(Timeout.default_class(), ErrorClass::Ambiguous);
        assert_eq!(ExternalFailure.default_class(), ErrorClass::Internal);
    }

    #[test]
    fn internal_class_is_not_retryable() {
        // Internal is not Transient, so is_retryable() must return false
        let err = DomainError::new(ErrorCode::Unavailable, "down").retryable(false);
        assert!(!err.is_retryable(), "Internal-classified error must not be retryable");
    }

    // AC-003/AC-004: Sensitive<T> redacts even a type with no Debug/Display impl.
    struct NoDebugOrDisplay;

    #[test]
    fn ac003_sensitive_debug_is_always_redacted() {
        assert_eq!(format!("{:?}", Sensitive(NoDebugOrDisplay)), "<redacted>");
        assert_eq!(format!("{:?}", Sensitive("plain str")), "<redacted>");
    }

    #[test]
    fn ac004_sensitive_display_is_always_redacted() {
        assert_eq!(format!("{}", Sensitive(NoDebugOrDisplay)), "<redacted>");
        assert_eq!(format!("{}", Sensitive("plain str")), "<redacted>");
    }

    // AC-001/AC-002: a compile-time bound proof that ErrorClass/ErrorCode are
    // Copy, distinct from the trybuild exhaustive-match fixtures (which prove
    // #[non_exhaustive], not Copy). Removing `Copy` from either derive still
    // leaves every other test in this module compiling and passing, since
    // Clone alone satisfies every existing use site -- only a bound assertion
    // like this one fails to compile without Copy.
    #[test]
    fn ac001_error_class_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<ErrorClass>();
    }

    #[test]
    fn ac002_error_code_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<ErrorCode>();
    }

    // Codes and classes are map keys (counting failures by code, routing by class).
    #[test]
    fn error_code_and_class_are_hashable_and_errors_compare() {
        let by_code: std::collections::HashSet<ErrorCode> = ErrorCode::ALL.into_iter().collect();
        assert_eq!(by_code.len(), ErrorCode::ALL.len());
        let classes: std::collections::HashSet<ErrorClass> =
            ErrorCode::ALL.iter().map(ErrorCode::default_class).collect();
        assert_eq!(classes.len(), 3);
        let a = DomainError::new(ErrorCode::NotFound, "m").hint("h");
        assert_eq!(a, a.clone());
        assert_ne!(a, a.clone().cause("c"));
    }

    // `is_retryable` and `class` cannot disagree: Transient exactly when retryable, for every code either way.
    #[test]
    fn is_retryable_is_exactly_the_transient_class() {
        for code in ErrorCode::ALL {
            for e in [DomainError::new(code, "m"), DomainError::new(code, "m").retryable(true)] {
                assert_eq!(e.is_retryable(), e.class() == ErrorClass::Transient, "{code:?}");
            }
            let off = DomainError::new(code, "m").retryable(false);
            assert!(!off.is_retryable());
            assert_eq!(off.class(), code.default_class(), "{code:?}");
        }
    }

    #[test]
    fn getters_read_what_the_builder_set() {
        let r = RecoveryHint::new("t");
        let e = DomainError::new(ErrorCode::Conflict, "m")
            .cause("c")
            .hint("h")
            .recovery(r.clone())
            .retry_after(Duration::from_secs(3));
        assert_eq!(e.code(), ErrorCode::Conflict);
        assert_eq!(e.message(), "m");
        assert_eq!(e.causes(), ["c"]);
        assert_eq!(e.hints(), [Hint::new("h")]);
        assert_eq!(e.get_recovery(), Some(&r));
        assert_eq!(e.get_retry_after(), Some(Duration::from_secs(3)));
        assert_eq!(e.retry_after(None).get_retry_after(), None, "None clears it");
    }

    #[test]
    fn context_prefixes_the_message_and_keeps_the_rest() {
        let e = DomainError::new(ErrorCode::Timeout, "no answer").cause("c").retry_after(Duration::from_secs(1));
        let wrapped = e.clone().context("step a").context("run r");
        assert_eq!(wrapped.message(), "run r: step a: no answer");
        assert_eq!((wrapped.code(), wrapped.causes(), wrapped.hints()), (e.code(), e.causes(), e.hints()));
        assert_eq!(wrapped.get_retry_after(), e.get_retry_after());
    }

    #[test]
    fn ac005_label_is_exact_snake_case_for_every_code() {
        use ErrorCode::*;
        assert_eq!(InvalidInput.label(), "invalid_input");
        assert_eq!(NotFound.label(), "not_found");
        assert_eq!(Unauthorized.label(), "unauthorized");
        assert_eq!(Forbidden.label(), "forbidden");
        assert_eq!(Conflict.label(), "conflict");
        assert_eq!(RateLimited.label(), "rate_limited");
        assert_eq!(Unavailable.label(), "unavailable");
        assert_eq!(Timeout.label(), "timeout");
        assert_eq!(ExternalFailure.label(), "external_failure");
    }

    // No wildcard arm: a new `ErrorCode` variant fails to compile here until it is placed in `ErrorCode::ALL`.
    #[test]
    fn error_code_all_lists_every_variant_once_and_from_str_inverts_label() {
        use ErrorCode::*;
        for (i, code) in ErrorCode::ALL.into_iter().enumerate() {
            let index = match code {
                InvalidInput => 0,
                NotFound => 1,
                Unauthorized => 2,
                Forbidden => 3,
                Conflict => 4,
                RateLimited => 5,
                Unavailable => 6,
                Timeout => 7,
                ExternalFailure => 8,
            };
            assert_eq!(index, i, "{code:?} is out of place in ErrorCode::ALL");
            assert_eq!(code.label().parse::<ErrorCode>(), Ok(code));
        }
        let err = "bogus_code".parse::<ErrorCode>().expect_err("not a label");
        assert_eq!(
            err.to_string(),
            "unknown error code \"bogus_code\"; expected one of: invalid_input, not_found, unauthorized, forbidden, \
             conflict, rate_limited, unavailable, timeout, external_failure"
        );
    }

    #[test]
    fn ac006_is_retryable_by_default_matches_the_not_processed_codes() {
        use ErrorCode::*;
        assert!(RateLimited.is_retryable_by_default());
        assert!(Unavailable.is_retryable_by_default());
        assert!(!Timeout.is_retryable_by_default(), "a timed-out request may have taken effect");
        assert!(!ExternalFailure.is_retryable_by_default(), "a downstream failure may have taken effect");
        assert!(!InvalidInput.is_retryable_by_default());
        assert!(!NotFound.is_retryable_by_default());
        assert!(!Unauthorized.is_retryable_by_default());
        assert!(!Forbidden.is_retryable_by_default());
        assert!(!Conflict.is_retryable_by_default());
    }

    #[test]
    fn ac008_new_defaults_hold_for_a_default_true_and_a_default_false_code() {
        let e = DomainError::new(ErrorCode::InvalidInput, "bad");
        assert_eq!(e.message, "bad");
        assert!(e.hints.is_empty());
        assert!(e.recovery.is_none());
        assert!(e.retry_after.is_none());
        assert_eq!(e.retryable, ErrorCode::InvalidInput.is_retryable_by_default());
        assert!(!e.retryable);

        let e = DomainError::new(ErrorCode::RateLimited, "slow down");
        assert_eq!(e.message, "slow down");
        assert!(e.hints.is_empty());
        assert!(e.recovery.is_none());
        assert!(e.retry_after.is_none());
        assert_eq!(e.retryable, ErrorCode::RateLimited.is_retryable_by_default());
        assert!(e.retryable);
    }

    #[test]
    fn ac009_hint_calls_are_stored_in_call_order() {
        let e = DomainError::new(ErrorCode::NotFound, "m").hint("a").hint("b");
        let as_strs: Vec<&str> = e.hints.iter().map(Hint::as_str).collect();
        assert_eq!(as_strs, vec!["a", "b"]);
    }

    #[test]
    fn ac010_recovery_overwrites_not_accumulates() {
        let r1 = RecoveryHint::new("first_tool");
        let r2 = RecoveryHint::new("second_tool");
        let e = DomainError::new(ErrorCode::NotFound, "m").recovery(r1).recovery(r2.clone());
        assert_eq!(e.recovery, Some(r2));
    }

    #[test]
    fn ac011_retryable_flips_in_both_directions() {
        // RateLimited defaults retryable=true; force it false.
        let e = DomainError::new(ErrorCode::RateLimited, "m").retryable(false);
        assert!(!e.retryable);

        // InvalidInput defaults retryable=false; force it true.
        let e = DomainError::new(ErrorCode::InvalidInput, "m").retryable(true);
        assert!(e.retryable);
    }

    #[test]
    fn ac012_retry_after_sets_the_exact_duration() {
        let d = Duration::from_millis(1234);
        let e = DomainError::new(ErrorCode::Unavailable, "m").retry_after(d);
        assert_eq!(e.retry_after, Some(d));
    }

    #[test]
    fn ac012a_retry_after_contributes_no_bytes_to_any_render_output() {
        let base = DomainError::new(ErrorCode::NotFound, "m").hint("h").recovery(RecoveryHint::new("t"));
        let with_retry_after = base.clone().retry_after(Duration::from_secs(30));
        assert_eq!(base.render(OutputFormat::Text), with_retry_after.render(OutputFormat::Text));
        assert_eq!(base.render_github_annotation(), with_retry_after.render_github_annotation());
        assert_eq!(base.render(OutputFormat::Json), with_retry_after.render(OutputFormat::Json));
    }

    #[test]
    fn ac013_exit_code_is_always_one() {
        assert_eq!(DomainError::new(ErrorCode::InvalidInput, "m").exit_code(), 1);
        assert_eq!(
            DomainError::new(ErrorCode::Unavailable, "m")
                .hint("h")
                .recovery(RecoveryHint::new("t"))
                .retryable(false)
                .retry_after(Duration::from_secs(5))
                .exit_code(),
            1
        );
    }

    #[test]
    fn ac014_render_with_hints_and_recovery_is_exact_literal() {
        let e = DomainError::new(ErrorCode::NotFound, "m").hint("h").recovery(RecoveryHint::new("t"));
        assert_eq!(
            e.render(OutputFormat::Text),
            "error: not_found\nmessage: m\nexit_code: 1\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  t,\"{}\",null"
        );
    }

    #[test]
    fn ac014b_render_with_recovery_and_no_hints_has_no_help_block() {
        let e = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t"));
        assert_eq!(
            e.render(OutputFormat::Text),
            "error: not_found\nmessage: m\nexit_code: 1\nrecovery[1]{tool,params,reason}:\n  t,\"{}\",null"
        );
        assert!(!e.render(OutputFormat::Text).contains("help["), "got: {}", e.render(OutputFormat::Text));
    }

    #[test]
    fn ac015_render_with_no_hints_no_recovery_is_exact_prefix_regardless_of_retry_after() {
        let e = DomainError::new(ErrorCode::NotFound, "m");
        assert_eq!(e.render(OutputFormat::Text), "error: not_found\nmessage: m\nexit_code: 1");

        let e_with_retry_after = e.retry_after(Duration::from_secs(1));
        assert_eq!(e_with_retry_after.render(OutputFormat::Text), "error: not_found\nmessage: m\nexit_code: 1");
    }

    // AC-014/AC-014a/AC-014b/AC-015: every prior exact-literal render()
    // witness uses a code whose retryable defaults to false (NotFound,
    // Conflict). The `.retryable` field is never rendered by render() at
    // all, so this closes the untested retryable==true half of the input
    // space for both the bare-prefix and hints+recovery shapes.
    #[test]
    fn ac015_render_with_no_hints_no_recovery_ignores_retryable_true() {
        let e = DomainError::new(ErrorCode::RateLimited, "m");
        assert!(e.retryable, "RateLimited defaults retryable=true");
        assert_eq!(e.render(OutputFormat::Text), "error: rate_limited\nmessage: m\nexit_code: 1");
    }

    #[test]
    fn ac014_render_with_hints_and_recovery_ignores_retryable_true() {
        let e = DomainError::new(ErrorCode::RateLimited, "m").hint("h").recovery(RecoveryHint::new("t"));
        assert_eq!(e.render(OutputFormat::Text), "error: rate_limited\nmessage: m\nexit_code: 1\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  t,\"{}\",null");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac018_render_json_top_level_keys_are_exactly_the_specified_set() {
        let with_recovery = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t"));
        let without_recovery = DomainError::new(ErrorCode::NotFound, "m");
        for err in [with_recovery, without_recovery] {
            let parsed: serde_json::Value = serde_json::from_str(&err.render(OutputFormat::Json)).unwrap();
            let obj = parsed.as_object().unwrap();
            let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, vec!["causes", "error", "hints", "isError", "message", "recovery", "retryable"]);
        }
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac018_render_json_field_values_match_the_source_fields() {
        let err = DomainError::new(ErrorCode::Unauthorized, "nope").hint("h1").retryable(true);
        let parsed: serde_json::Value = serde_json::from_str(&err.render(OutputFormat::Json)).unwrap();
        assert_eq!(parsed["isError"], serde_json::json!(true));
        assert_eq!(parsed["error"], serde_json::json!("unauthorized"));
        assert_eq!(parsed["message"], serde_json::json!("nope"));
        assert_eq!(parsed["retryable"], serde_json::json!(true));
        assert_eq!(parsed["hints"], serde_json::json!(["h1"]));
        assert_eq!(parsed["recovery"], serde_json::Value::Null);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac020_reason_key_is_absent_when_none_and_present_when_some() {
        let no_reason = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t"));
        let parsed: serde_json::Value = serde_json::from_str(&no_reason.render(OutputFormat::Json)).unwrap();
        assert!(parsed["recovery"].as_object().unwrap().get("reason").is_none(), "got: {parsed}");

        let with_reason = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t").reason("why"));
        let parsed: serde_json::Value = serde_json::from_str(&with_reason.render(OutputFormat::Json)).unwrap();
        assert_eq!(parsed["recovery"]["reason"], serde_json::json!("why"));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac021_quote_and_backslash_round_trip_exactly() {
        let original = r#"has "quotes" and \backslash"#;
        let err = DomainError::new(ErrorCode::NotFound, original);
        let parsed: serde_json::Value = serde_json::from_str(&err.render(OutputFormat::Json)).unwrap();
        assert_eq!(parsed["message"].as_str().unwrap(), original);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac021a_newline_cr_and_tab_round_trip_exactly_in_message_and_hint() {
        let original_message = "line1\nline2\rline3\ttabbed";
        let original_hint = "hint\nwith\rcontrol\tchars";
        let err = DomainError::new(ErrorCode::NotFound, original_message).hint(original_hint);
        let raw = err.render(OutputFormat::Json);
        assert!(!raw.contains('\n') && !raw.contains('\r') && !raw.contains('\t'), "got: {raw:?}");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["message"].as_str().unwrap(), original_message);
        assert_eq!(parsed["hints"][0].as_str().unwrap(), original_hint);
    }

    // AC-018/AC-021a: \n, \r, \t each have their own dedicated escape arm in
    // json::push_str; this proves the generic below-0x20 control-character
    // arm (\u00XX) that every OTHER control character falls through to.
    #[test]
    #[cfg(feature = "serde")]
    fn ac021a_generic_control_characters_below_0x20_round_trip_exactly() {
        let original = "before\u{1}mid\u{1f}end";
        let err = DomainError::new(ErrorCode::NotFound, original);
        let raw = err.render(OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["message"].as_str().unwrap(), original);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac021b_recovery_tool_param_key_and_reason_round_trip_exactly() {
        let original_tool = r#"tool "name" with \backslash"#;
        let original_key = r#"key "with" \quotes"#;
        let original_reason = r#"reason "text" with \backslash"#;
        let r = RecoveryHint::new(original_tool)
            .param(original_key, crate::kv::KvValue::Text("v".to_string()))
            .reason(original_reason);
        let err = DomainError::new(ErrorCode::NotFound, "m").recovery(r);
        let parsed: serde_json::Value = serde_json::from_str(&err.render(OutputFormat::Json)).unwrap();
        assert_eq!(parsed["recovery"]["tool"].as_str().unwrap(), original_tool);
        assert_eq!(parsed["recovery"]["reason"].as_str().unwrap(), original_reason);
        let params = parsed["recovery"]["params"].as_object().unwrap();
        assert!(params.contains_key(original_key), "got keys: {:?}", params.keys().collect::<Vec<_>>());
    }

    #[test]
    fn ac022_to_call_tool_result_matches_render_outputs_exactly() {
        let err = DomainError::new(ErrorCode::Unavailable, "down").hint("h");
        let result = err.to_call_tool_result();
        assert!(result.is_error);
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].text, err.render(OutputFormat::Text));
        assert_eq!(result.content[0].audience, vec![crate::audience::Audience::Assistant]);
        assert_eq!(
            result.structured_content.as_ref().map(|j| j.as_str()),
            Some(err.render(OutputFormat::Json).as_str())
        );
    }

    #[test]
    fn ac031_is_retryable_true_direction() {
        let err = DomainError::new(ErrorCode::Unavailable, "down").retryable(true);
        assert!(err.is_retryable(), "Transient-classified error must be retryable");
    }

    #[test]
    fn ac032_all_transient_default_codes_are_transient_and_retryable() {
        use ErrorCode::*;
        for code in [RateLimited, Unavailable] {
            let err = DomainError::new(code, "m");
            assert_eq!(err.class(), ErrorClass::Transient, "{code:?}");
            assert!(err.is_retryable(), "{code:?}");
        }
    }

    // A timeout or downstream failure may have applied the request (RFC 9110 §15.6), so by default it must not tell
    // an agent that retrying is safe. Callers that know the operation is idempotent opt in with `.retryable(true)`.
    #[test]
    fn ambiguous_codes_do_not_claim_retry_is_safe_by_default() {
        for code in [ErrorCode::Timeout, ErrorCode::ExternalFailure] {
            let d = DomainError::new(code, "POST /charge timed out");
            assert!(!d.retryable, "{code:?}");
            assert!(d.render(OutputFormat::Json).contains("\"retryable\":false"), "{code:?}");
            assert!(!d.is_retryable(), "{code:?}");
            let idempotent = DomainError::new(code, "GET /items timed out").retryable(true);
            assert_eq!(idempotent.class(), ErrorClass::Transient, "{code:?} opt-in");
            assert!(idempotent.is_retryable(), "{code:?} opt-in");
        }
    }

    // A timeout is its own class, not an internal error, and says to verify before retrying until the caller marks
    // the operation idempotent. A broader ExternalFailure (a failed local walk, say) gets no such hint.
    #[test]
    fn a_timeout_is_ambiguous_and_says_to_verify_first() {
        let t = DomainError::new(ErrorCode::Timeout, "POST /charge timed out");
        assert_eq!(t.hints.iter().map(Hint::as_str).collect::<Vec<_>>(), [MAY_HAVE_TAKEN_EFFECT_HINT]);
        assert_eq!(t.class(), ErrorClass::Ambiguous);
        let idempotent = DomainError::new(ErrorCode::Timeout, "GET /items timed out").retryable(true);
        assert!(idempotent.hints.is_empty(), "an idempotent operation needs no verify-first hint");
        let back = DomainError::new(ErrorCode::Timeout, "x").retryable(true).retryable(false);
        assert_eq!(back.hints.len(), 1, "unmarking restores the hint once");
        let walk = DomainError::new(ErrorCode::ExternalFailure, "walk failed");
        assert!(walk.hints.is_empty());
        assert!(!walk.retryable);
        assert_eq!(walk.class(), ErrorClass::Internal);
    }

    #[test]
    fn ac033_all_user_default_codes_are_user_and_not_retryable() {
        use ErrorCode::*;
        for code in [InvalidInput, NotFound, Unauthorized, Forbidden, Conflict] {
            let err = DomainError::new(code, "m");
            assert_eq!(err.class(), ErrorClass::User, "{code:?}");
            assert!(!err.is_retryable(), "{code:?}");
        }
    }

    #[test]
    fn ac034_display_impls_agree_on_the_exact_format() {
        let d = DomainError::new(ErrorCode::RateLimited, "slow down");
        assert_eq!(format!("{d}"), "rate_limited: slow down");
        assert_eq!(format!("{}", ErrorCode::RateLimited), "rate_limited");
    }

    // A repeated param key must not produce a JSON object with a duplicate key (parsers keep one value silently).
    #[test]
    fn render_json_never_repeats_a_recovery_param_key() {
        let mut r = RecoveryHint::new("t");
        r.params.push(("x".to_string(), crate::kv::KvValue::Int(1)));
        r.params.push(("x".to_string(), crate::kv::KvValue::Int(2)));
        let json = DomainError::new(ErrorCode::NotFound, "m").recovery(r).render(OutputFormat::Json);
        assert!(json.contains("\"params\":{\"x\":2}"), "got: {json}");
    }

    #[test]
    fn ac035_render_json_recovery_params_preserve_insertion_order() {
        let r = RecoveryHint::new("t")
            .param("zebra", crate::kv::KvValue::Int(1))
            .param("alpha", crate::kv::KvValue::Int(2));
        let e = DomainError::new(ErrorCode::NotFound, "m").recovery(r);
        let json = e.render(OutputFormat::Json);
        let zebra_pos = json.find("\"zebra\"").expect("zebra key present");
        let alpha_pos = json.find("\"alpha\"").expect("alpha key present");
        assert!(zebra_pos < alpha_pos, "zebra (added first) must appear before alpha, got: {json}");
    }

    // AC-018: the existing multi-element witnesses above only byte-scan for
    // substrings/relative offsets, which a missing comma separator (in either
    // the hints loop or the params loop) does not perturb. Actually parse the
    // output to prove both element-separator call sites are present.
    #[test]
    #[cfg(feature = "serde")]
    fn ac018_render_json_is_valid_with_multiple_hints_and_multiple_params() {
        let r = RecoveryHint::new("t")
            .param("zebra", crate::kv::KvValue::Int(1))
            .param("alpha", crate::kv::KvValue::Int(2));
        let e = DomainError::new(ErrorCode::NotFound, "m").hint("a").hint("b").recovery(r);
        let raw = e.render(OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["hints"], serde_json::json!(["a", "b"]));
        assert_eq!(parsed["recovery"]["params"], serde_json::json!({"zebra": 1, "alpha": 2}));
    }
}
