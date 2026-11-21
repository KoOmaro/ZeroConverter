use serde::Serialize;
use thiserror::Error;

use crate::jobs::{JobFailure, JobFailureKind, JobStage};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Internal core error: {0}")]
    Internal(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Probe error: {0}")]
    Probe(String),
    #[error("Validation error: {0}")]
    Validate(String),
    #[error("Execution error: {0}")]
    Execute(String),
    #[error("Registry error: {0}")]
    Registry(String),
    /// The user stopped it. Not a failure: never shown as an error. The
    /// text is what the frontend checks for.
    #[error("CANCELLED")]
    Cancelled,
    /// ICO output larger than an icon may be. A validation error with the
    /// planned size, so callers can offer a fit-to-256 resize.
    #[error("Validation error: ICO output requires dimensions up to {max}x{max}. Current planned dimensions: {width}x{height}. Use resize to fit within {max}x{max}.")]
    IcoTooLarge { width: u32, height: u32, max: u32 },
    /// The input is over the large-file limit and the request did not carry
    /// the user's consent (`confirm_large`).
    #[error("This file is {} MB. Files over 100 MB convert only after you confirm.", .bytes / (1024 * 1024))]
    ConsentRequired { bytes: u64 },
}

impl AppError {
    /// Stable, machine-readable code (`docs/CLI.md`). Codes are only ever
    /// added, never renamed; the message beside them is for people.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Internal(_) => "internal",
            AppError::Io(_) => "io",
            AppError::Probe(_) => "unsupported_input",
            AppError::Validate(_) | AppError::IcoTooLarge { .. } => "invalid_request",
            AppError::Execute(_) => "conversion_failed",
            AppError::Registry(_) => "no_route",
            AppError::Cancelled => "cancelled",
            AppError::ConsentRequired { .. } => "consent_required",
        }
    }

    pub fn to_failure(&self, stage: JobStage) -> JobFailure {
        let kind = match self {
            AppError::Io(_) => JobFailureKind::Io,
            AppError::Validate(_)
            | AppError::IcoTooLarge { .. }
            | AppError::ConsentRequired { .. } => JobFailureKind::Validation,
            AppError::Probe(_) => JobFailureKind::Format,
            AppError::Execute(_) => JobFailureKind::Decode,
            AppError::Cancelled => JobFailureKind::Internal,
            _ => JobFailureKind::Internal,
        };

        JobFailure {
            stage,
            kind,
            message: self.to_string(),
        }
    }
}

// Implement Serialize so it can be returned by Tauri commands
// Internal taxonomy remains separate but command-facing errors are safely serializable
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        let all = [
            (AppError::Internal(String::new()), "internal"),
            (AppError::Io(String::new()), "io"),
            (AppError::Probe(String::new()), "unsupported_input"),
            (AppError::Validate(String::new()), "invalid_request"),
            (
                AppError::IcoTooLarge {
                    width: 0,
                    height: 0,
                    max: 256,
                },
                "invalid_request",
            ),
            (AppError::Execute(String::new()), "conversion_failed"),
            (AppError::Registry(String::new()), "no_route"),
            (AppError::Cancelled, "cancelled"),
            (AppError::ConsentRequired { bytes: 0 }, "consent_required"),
        ];
        for (err, code) in all {
            assert_eq!(err.code(), code);
        }
    }

    // ═══════════════════════════════════════════
    // Display Trait Tests
    // ═══════════════════════════════════════════

    #[test]
    fn internal_error_display() {
        let err = AppError::Internal("core meltdown".to_string());
        assert!(err.to_string().contains("core meltdown"));
        assert!(err.to_string().contains("Internal"));
    }

    #[test]
    fn io_error_display() {
        let err = AppError::Io("file not found".to_string());
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn conversion_error_display() {
        let err = AppError::Registry("invalid pipeline".to_string());
        assert!(err.to_string().contains("invalid pipeline"));
    }

    #[test]
    fn probe_error_display() {
        let err = AppError::Probe("bad signature".to_string());
        assert!(err.to_string().contains("bad signature"));
    }

    #[test]
    fn validate_error_display() {
        let err = AppError::Validate("missing field".to_string());
        assert!(err.to_string().contains("missing field"));
    }

    #[test]
    fn execute_error_display() {
        let err = AppError::Execute("codec failure".to_string());
        assert!(err.to_string().contains("codec failure"));
    }

    #[test]
    fn registry_error_display() {
        let err = AppError::Registry("handler not found".to_string());
        assert!(err.to_string().contains("handler not found"));
    }

    // ═══════════════════════════════════════════
    // Serialization Safety (Tauri boundary)
    // ═══════════════════════════════════════════

    #[test]
    fn cancelled_reads_as_the_word_the_frontend_checks() {
        assert_eq!(
            serde_json::to_string(&AppError::Cancelled).unwrap(),
            "\"CANCELLED\""
        );
    }

    #[test]
    fn all_variants_serialize_to_json_without_panic() {
        let variants: Vec<AppError> = vec![
            AppError::Internal("test".to_string()),
            AppError::Io("test".to_string()),
            AppError::Probe("test".to_string()),
            AppError::Validate("test".to_string()),
            AppError::Execute("test".to_string()),
            AppError::Registry("test".to_string()),
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant);
            assert!(json.is_ok(), "Failed to serialize: {:?}", variant);
            let json_str = json.unwrap();
            assert!(!json_str.is_empty());
            assert!(json_str.contains("test"));
        }
    }

    #[test]
    fn serialization_produces_string_not_object() {
        // AppError serializes as a plain string, not a structured object
        let err = AppError::Probe("signature mismatch".to_string());
        let json = serde_json::to_value(&err).unwrap();
        assert!(
            json.is_string(),
            "Expected string serialization, got: {:?}",
            json
        );
    }

    // ═══════════════════════════════════════════
    // Error Messages Are Distinguishable
    // ═══════════════════════════════════════════

    #[test]
    fn different_variants_produce_different_prefixes() {
        let io = AppError::Io("shared_msg".to_string()).to_string();
        let probe = AppError::Probe("shared_msg".to_string()).to_string();
        let validate = AppError::Validate("shared_msg".to_string()).to_string();
        // Same message, different prefix
        assert_ne!(io, probe);
        assert_ne!(probe, validate);
        assert_ne!(io, validate);
    }

    // ═══════════════════════════════════════════
    // Edge Cases
    // ═══════════════════════════════════════════

    #[test]
    fn empty_message_does_not_panic() {
        let err = AppError::Internal("".to_string());
        let _ = err.to_string();
        let _ = serde_json::to_string(&err).unwrap();
    }

    #[test]
    fn unicode_message_serializes_correctly() {
        let err = AppError::Execute("Dönüşüm başarısız: 日本語テスト".to_string());
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("Dönüşüm"));
        assert!(json.contains("日本語"));
    }

    #[test]
    fn very_long_message_serializes() {
        let long_msg = "x".repeat(10_000);
        let err = AppError::Internal(long_msg.clone());
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.len() > 10_000);
    }
}
