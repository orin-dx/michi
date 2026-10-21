use std::time::Duration;

use crate::hints::Hint;
use crate::recovery::RecoveryHint;

/// Classification of a `crate::Error` for routing and display decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
    /// A transient failure expected to resolve without intervention.
    Transient,
}

/// Wraps a value so its inner content is omitted from `Debug` and `Display`.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

    /// Whether this code is conventionally retryable.
    #[must_use]
    pub fn is_retryable_by_default(&self) -> bool {
        matches!(self, Self::RateLimited | Self::Unavailable | Self::Timeout | Self::ExternalFailure)
    }

    /// The default [`ErrorClass`] for this code, independent of `DomainError.retryable`.
    ///
    /// Infrastructure codes (`RateLimited`, `Unavailable`, `Timeout`,
    /// `ExternalFailure`) return `Internal`; caller-fault codes return `User`.
    /// Used by [`Error::class()`].
    #[must_use]
    pub fn default_class(&self) -> ErrorClass {
        match self {
            Self::InvalidInput | Self::NotFound | Self::Unauthorized | Self::Forbidden | Self::Conflict => {
                ErrorClass::User
            }
            Self::RateLimited | Self::Unavailable | Self::Timeout | Self::ExternalFailure => ErrorClass::Internal,
        }
    }
}

/// A domain-level error.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct DomainError {
    /// The error classification.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Contextual hints, rendered as a trailing `help[N]:` block.
    pub hints: Vec<Hint>,
    /// Optional structured recovery hint.
    pub recovery: Option<RecoveryHint>,
    /// Whether this error is safe to retry.
    pub retryable: bool,
    /// Parsed `Retry-After` delay, if known.
    pub retry_after: Option<Duration>,
}

impl DomainError {
    /// Create a domain error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            retryable: code.is_retryable_by_default(),
            code,
            message: message.into(),
            hints: Vec::new(),
            recovery: None,
            retry_after: None,
        }
    }

    /// Append a contextual hint.
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

    /// Override the default retryability for this error instance.
    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Attach a `Retry-After` delay.
    #[must_use]
    pub fn retry_after(mut self, d: Duration) -> Self {
        self.retry_after = Some(d);
        self
    }

    /// The process exit code to use when this error is fatal. Always `1`.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        1
    }

    /// Render as a TOON 4.4 root object, with no trailing newline: `error: <code label>`, `message: …` (quoted per
    /// §7.2), `exit_code: 1`, then the inline `help[N]: …` array and the `recovery[1]{tool,params,reason}:` table when
    /// present.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(96 + self.message.len() + self.hints.len() * 50);
        push_error_fields(&mut out, self.code.label(), &self.message);
        crate::hints::append_hints(&mut out, &self.hints);
        if let Some(r) = &self.recovery {
            crate::recovery::append_recovery(&mut out, std::slice::from_ref(r));
        }
        out
    }

    /// Render formatted for GitHub Actions workflow annotations (`::error title=code::message`).
    ///
    /// Newlines and carriage returns are percent-encoded (`%0A`, `%0D`) to
    /// keep the annotation on a single line as required by the Actions runner.
    #[must_use]
    pub fn render_github_annotation(&self) -> String {
        format!("::error title={}::{}", self.code.label(), self.message.replace('\n', "%0A").replace('\r', "%0D"))
    }

    /// Render as a JSON object matching `AgentResponse::render_json()` shape.
    ///
    /// Keys: `isError`, `error`, `message`, `retryable`, `hints`, `recovery`.
    /// Client code reading `structured_content` should not need two schemas.
    #[must_use]
    pub fn render_json(&self) -> String {
        let hints_est: usize = self.hints.iter().map(|h| h.as_str().len() + 4).sum();
        let recovery_est: usize = self.recovery.as_ref().map_or(0, |r| {
            50 + r.tool.len()
                + r.params.iter().map(|_| 20usize).sum::<usize>()
                + r.reason.as_ref().map_or(0, |s| s.len() + 12)
        });
        let mut out = String::with_capacity(128 + self.message.len() + hints_est + recovery_est);
        out.push_str("{\"isError\":true,\"error\":");
        crate::kv::json_escape_str(&mut out, self.code.label());
        out.push_str(",\"message\":");
        crate::kv::json_escape_str(&mut out, &self.message);
        out.push_str(",\"retryable\":");
        out.push_str(if self.retryable { "true" } else { "false" });
        out.push_str(",\"hints\":[");
        for (i, h) in self.hints.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            crate::kv::json_escape_str(&mut out, h.as_str());
        }
        out.push_str("],\"recovery\":");
        match &self.recovery {
            None => out.push_str("null"),
            Some(r) => {
                out.push_str("{\"tool\":");
                crate::kv::json_escape_str(&mut out, &r.tool);
                out.push_str(",\"params\":{");
                for (j, (k, v)) in r.params.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    crate::kv::json_escape_str(&mut out, k);
                    out.push(':');
                    crate::kv::kv_value_to_json(&mut out, v);
                }
                out.push('}');
                if let Some(reason) = &r.reason {
                    out.push_str(",\"reason\":");
                    crate::kv::json_escape_str(&mut out, reason);
                }
                out.push('}');
            }
        }
        out.push('}');
        out
    }

    /// Build an MCP [`CallToolResult`](crate::mcp::CallToolResult) representing this error.
    ///
    /// Sets `is_error: true`. Text content uses [`DomainError::render()`];
    /// `structured_content` uses [`DomainError::render_json()`].
    ///
    /// **Canonical path:** call this directly when you hold a `DomainError`.
    /// Do not wrap in `AgentResponse::as_error()` — that loses typed fields.
    #[must_use]
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        crate::mcp::CallToolResult {
            content: vec![crate::mcp::ContentBlock {
                text: self.render(),
                audience: vec![crate::audience::Audience::Assistant],
            }],
            is_error: true,
            structured_content: self.render_json(),
        }
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.label(), self.message)
    }
}

impl std::error::Error for DomainError {}

/// The fields every rendered error document starts with: `error: <code>`, `message: …`, `exit_code: 1`.
fn push_error_fields(out: &mut String, code: &str, message: &str) {
    out.push_str("error: ");
    out.push_str(code);
    out.push_str("\nmessage: ");
    out.push_str(&crate::toon::escape_value(message));
    out.push_str("\nexit_code: 1");
}

#[cfg(feature = "miette")]
impl miette::Diagnostic for DomainError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.code.label()))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        if self.hints.is_empty() {
            None
        } else {
            Some(Box::new(self.hints.iter().map(|h| h.as_str()).collect::<Vec<_>>().join("\n")))
        }
    }
}

/// The unified error type for the michi crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid or malformed input.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// A required resource was not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// A classified domain error with hints, recovery, and retry metadata.
    #[error("{}: {}", .0.code.label(), .0.message)]
    Domain(#[from] DomainError),
}

impl Error {
    /// Render this error as a TOON 4.4 document in the shape of [`DomainError::render`]: `Domain` renders its inner
    /// error; `InvalidInput(m)` and `NotFound(m)` render `error: invalid_input` / `error: not_found`, `message: m`
    /// (quoted per §7.2) and `exit_code: 1`, with no trailing newline.
    #[must_use]
    pub fn render(&self) -> String {
        let (code, message) = match self {
            Self::Domain(d) => return d.render(),
            Self::InvalidInput(m) => (ErrorCode::InvalidInput.label(), m),
            Self::NotFound(m) => (ErrorCode::NotFound.label(), m),
        };
        let mut out = String::with_capacity(48 + message.len());
        push_error_fields(&mut out, code, message);
        out
    }

    /// The process exit code to use when this error is fatal.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        1
    }

    /// Classify this error for routing decisions.
    #[must_use]
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Domain(d) if d.retryable => ErrorClass::Transient,
            Self::Domain(d) => d.code.default_class(),
            Self::InvalidInput(_) | Self::NotFound(_) => ErrorClass::User,
        }
    }

    /// Whether this error is safe to retry automatically.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self.class(), ErrorClass::Transient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_error_render_json_basic() {
        let err = DomainError::new(ErrorCode::NotFound, "Item 42 not found");
        let json = err.render_json();
        assert!(json.contains("\"isError\":true"), "got: {json}");
        assert!(json.contains("\"error\":"), "got: {json}");
        assert!(json.contains("\"message\":\"Item 42 not found\""), "got: {json}");
        assert!(json.contains("\"retryable\":false"), "got: {json}");
        assert!(json.contains("\"hints\":["), "got: {json}");
    }

    #[test]
    fn domain_error_render_json_includes_hints() {
        let err = DomainError::new(ErrorCode::NotFound, "not found").hint("Try searching with get_items");
        let json = err.render_json();
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
        let json = err.render_json();
        assert!(json.contains("\\\"name\\\""), "quotes in message must be JSON-escaped, got: {json}");
    }

    #[test]
    fn invalid_input_renders() {
        let e = Error::InvalidInput("field 'name' is required".into());
        assert_eq!(e.render(), "error: invalid_input\nmessage: field 'name' is required\nexit_code: 1");
    }

    #[test]
    fn domain_error_renders_kv_block_with_hints() {
        let e = DomainError::new(ErrorCode::NotFound, "Issue #9999 does not exist in this repository")
            .hint("Call list_issues to see available numbers");
        let out = e.render();
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
        let json = err.render_json();
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
        let json = err.render_json();
        // Smoke test: no panic = capacity was sufficient (Vec didn't reallocate unexpectedly).
        // JSON correctness is validated by structure checks.
        assert!(json.contains("\"hints\":["), "got: {json}");
        assert!(json.contains("\"recovery\":{"), "got: {json}");
        assert!(json.contains("\"reason\":"), "got: {json}");
    }

    #[test]
    fn unavailable_with_retryable_false_is_internal_not_user() {
        let err = Error::Domain(DomainError::new(ErrorCode::Unavailable, "down").retryable(false));
        assert_eq!(err.class(), ErrorClass::Internal, "infra error with retryable=false must be Internal, not User");
    }

    #[test]
    fn invalid_input_is_user() {
        let err = Error::Domain(DomainError::new(ErrorCode::InvalidInput, "bad input"));
        assert_eq!(err.class(), ErrorClass::User);
    }

    #[test]
    fn rate_limited_retryable_true_is_transient() {
        // Default: RateLimited is retryable by default
        let err = Error::Domain(DomainError::new(ErrorCode::RateLimited, "429"));
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
        assert_eq!(Timeout.default_class(), ErrorClass::Internal);
        assert_eq!(ExternalFailure.default_class(), ErrorClass::Internal);
    }

    #[test]
    fn internal_class_is_not_retryable() {
        // Internal is not Transient, so is_retryable() must return false
        let err = Error::Domain(DomainError::new(ErrorCode::Unavailable, "down").retryable(false));
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

    #[test]
    fn ac006_is_retryable_by_default_matches_the_transient_codes() {
        use ErrorCode::*;
        assert!(RateLimited.is_retryable_by_default());
        assert!(Unavailable.is_retryable_by_default());
        assert!(Timeout.is_retryable_by_default());
        assert!(ExternalFailure.is_retryable_by_default());
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
        assert_eq!(base.render(), with_retry_after.render());
        assert_eq!(base.render_github_annotation(), with_retry_after.render_github_annotation());
        assert_eq!(base.render_json(), with_retry_after.render_json());
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
            e.render(),
            "error: not_found\nmessage: m\nexit_code: 1\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  t,\"{}\",null"
        );
    }

    #[test]
    fn ac014b_render_with_recovery_and_no_hints_has_no_help_block() {
        let e = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t"));
        assert_eq!(
            e.render(),
            "error: not_found\nmessage: m\nexit_code: 1\nrecovery[1]{tool,params,reason}:\n  t,\"{}\",null"
        );
        assert!(!e.render().contains("help["), "got: {}", e.render());
    }

    #[test]
    fn ac015_render_with_no_hints_no_recovery_is_exact_prefix_regardless_of_retry_after() {
        let e = DomainError::new(ErrorCode::NotFound, "m");
        assert_eq!(e.render(), "error: not_found\nmessage: m\nexit_code: 1");

        let e_with_retry_after = e.retry_after(Duration::from_secs(1));
        assert_eq!(e_with_retry_after.render(), "error: not_found\nmessage: m\nexit_code: 1");
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
        assert_eq!(e.render(), "error: rate_limited\nmessage: m\nexit_code: 1");
    }

    #[test]
    fn ac014_render_with_hints_and_recovery_ignores_retryable_true() {
        let e = DomainError::new(ErrorCode::RateLimited, "m").hint("h").recovery(RecoveryHint::new("t"));
        assert_eq!(e.render(), "error: rate_limited\nmessage: m\nexit_code: 1\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  t,\"{}\",null");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac018_render_json_top_level_keys_are_exactly_the_specified_set() {
        let with_recovery = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t"));
        let without_recovery = DomainError::new(ErrorCode::NotFound, "m");
        for err in [with_recovery, without_recovery] {
            let parsed: serde_json::Value = serde_json::from_str(&err.render_json()).unwrap();
            let obj = parsed.as_object().unwrap();
            let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, vec!["error", "hints", "isError", "message", "recovery", "retryable"]);
        }
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac018_render_json_field_values_match_the_source_fields() {
        let err = DomainError::new(ErrorCode::Unauthorized, "nope").hint("h1").retryable(true);
        let parsed: serde_json::Value = serde_json::from_str(&err.render_json()).unwrap();
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
        let parsed: serde_json::Value = serde_json::from_str(&no_reason.render_json()).unwrap();
        assert!(parsed["recovery"].as_object().unwrap().get("reason").is_none(), "got: {parsed}");

        let with_reason = DomainError::new(ErrorCode::NotFound, "m").recovery(RecoveryHint::new("t").reason("why"));
        let parsed: serde_json::Value = serde_json::from_str(&with_reason.render_json()).unwrap();
        assert_eq!(parsed["recovery"]["reason"], serde_json::json!("why"));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac021_quote_and_backslash_round_trip_exactly() {
        let original = r#"has "quotes" and \backslash"#;
        let err = DomainError::new(ErrorCode::NotFound, original);
        let parsed: serde_json::Value = serde_json::from_str(&err.render_json()).unwrap();
        assert_eq!(parsed["message"].as_str().unwrap(), original);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac021a_newline_cr_and_tab_round_trip_exactly_in_message_and_hint() {
        let original_message = "line1\nline2\rline3\ttabbed";
        let original_hint = "hint\nwith\rcontrol\tchars";
        let err = DomainError::new(ErrorCode::NotFound, original_message).hint(original_hint);
        let raw = err.render_json();
        assert!(!raw.contains('\n') && !raw.contains('\r') && !raw.contains('\t'), "got: {raw:?}");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["message"].as_str().unwrap(), original_message);
        assert_eq!(parsed["hints"][0].as_str().unwrap(), original_hint);
    }

    // AC-018/AC-021a: \n, \r, \t each have their own dedicated escape arm in
    // json_escape_str; this proves the generic below-0x20 control-character
    // arm (\u00XX) that every OTHER control character falls through to.
    #[test]
    #[cfg(feature = "serde")]
    fn ac021a_generic_control_characters_below_0x20_round_trip_exactly() {
        let original = "before\u{1}mid\u{1f}end";
        let err = DomainError::new(ErrorCode::NotFound, original);
        let raw = err.render_json();
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
        let parsed: serde_json::Value = serde_json::from_str(&err.render_json()).unwrap();
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
        assert_eq!(result.content[0].text, err.render());
        assert_eq!(result.content[0].audience, vec![crate::audience::Audience::Assistant]);
        assert_eq!(result.structured_content, err.render_json());
    }

    #[test]
    fn ac024_error_domain_render_matches_inner_domain_error_render() {
        let d = DomainError::new(ErrorCode::Conflict, "m").hint("h");
        assert_eq!(Error::Domain(d.clone()).render(), d.render());
    }

    #[test]
    fn ac026_error_not_found_variant_renders_exact_prefix() {
        let e = Error::NotFound("issue 42".into());
        assert_eq!(e.render(), "error: not_found\nmessage: issue 42\nexit_code: 1");
    }

    #[test]
    fn ac027_exit_code_is_one_for_every_variant() {
        assert_eq!(Error::InvalidInput("x".into()).exit_code(), 1);
        assert_eq!(Error::NotFound("x".into()).exit_code(), 1);
        assert_eq!(Error::Domain(DomainError::new(ErrorCode::Unavailable, "x")).exit_code(), 1);
    }

    // AC-030 TRAP: a test named for the InvalidInput/NotFound *variants* must actually
    // construct those variants, not Error::Domain(DomainError::new(InvalidInput, ..)) --
    // the two are different Error variants with independently-implemented class() arms.
    #[test]
    fn ac030_invalid_input_and_not_found_variants_are_always_user_class() {
        assert_eq!(Error::InvalidInput(String::new()).class(), ErrorClass::User);
        assert_eq!(Error::InvalidInput("msg".into()).class(), ErrorClass::User);
        assert_eq!(Error::NotFound(String::new()).class(), ErrorClass::User);
        assert_eq!(Error::NotFound("msg".into()).class(), ErrorClass::User);
    }

    #[test]
    fn ac031_is_retryable_true_direction() {
        let err = Error::Domain(DomainError::new(ErrorCode::Unavailable, "down").retryable(true));
        assert!(err.is_retryable(), "Transient-classified error must be retryable");
    }

    #[test]
    fn ac031_invalid_input_and_not_found_variants_are_never_retryable() {
        assert!(!Error::InvalidInput(String::new()).is_retryable());
        assert!(!Error::InvalidInput("msg".into()).is_retryable());
        assert!(!Error::NotFound(String::new()).is_retryable());
        assert!(!Error::NotFound("msg".into()).is_retryable());
    }

    #[test]
    fn ac032_all_transient_default_codes_are_transient_and_retryable() {
        use ErrorCode::*;
        for code in [RateLimited, Unavailable, Timeout, ExternalFailure] {
            let err = Error::Domain(DomainError::new(code, "m"));
            assert_eq!(err.class(), ErrorClass::Transient, "{code:?}");
            assert!(err.is_retryable(), "{code:?}");
        }
    }

    #[test]
    fn ac033_all_user_default_codes_are_user_and_not_retryable() {
        use ErrorCode::*;
        for code in [InvalidInput, NotFound, Unauthorized, Forbidden, Conflict] {
            let err = Error::Domain(DomainError::new(code, "m"));
            assert_eq!(err.class(), ErrorClass::User, "{code:?}");
            assert!(!err.is_retryable(), "{code:?}");
        }
    }

    #[test]
    fn ac034_display_impls_agree_on_the_exact_format() {
        let d = DomainError::new(ErrorCode::RateLimited, "slow down");
        assert_eq!(format!("{d}"), "rate_limited: slow down");
        let e = Error::Domain(d);
        assert_eq!(format!("{e}"), "rate_limited: slow down");
    }

    #[test]
    fn ac035_render_json_recovery_params_preserve_insertion_order() {
        let r = RecoveryHint::new("t")
            .param("zebra", crate::kv::KvValue::Int(1))
            .param("alpha", crate::kv::KvValue::Int(2));
        let e = DomainError::new(ErrorCode::NotFound, "m").recovery(r);
        let json = e.render_json();
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
        let raw = e.render_json();
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["hints"], serde_json::json!(["a", "b"]));
        assert_eq!(parsed["recovery"]["params"], serde_json::json!({"zebra": 1, "alpha": 2}));
    }
}
