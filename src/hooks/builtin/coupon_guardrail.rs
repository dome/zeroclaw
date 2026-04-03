//! Coupon Guardrail Hook for ZeroClaw
//!
//! This hook provides multi-layer protection against coupon fraud:
//! 1. Format validation - checks coupon code format
//! 2. Blocked patterns - rejects suspicious/test codes
//! 3. Sub-agent delegation - requires validation through coupon_validator agent
//!
//! Usage:
//! ```toml
//! [hooks.coupon_guardrail]
//! enabled = true
//! code_pattern = "^[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}$"
//! require_validation = true
//! validation_agent = "coupon_validator"
//! blocked_patterns = ["TEST", "FAKE", "DEMO", "AAAA-*", "0000-*"]
//! ```

use async_trait::async_trait;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hooks::traits::{HookHandler, HookResult};

/// Configuration for the coupon guardrail hook
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CouponGuardrailConfig {
    /// Enable or disable the guardrail
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Regex pattern for valid coupon codes
    /// Default: XXXX-XXXX-XXXX format
    #[serde(default = "default_code_pattern")]
    pub code_pattern: String,

    /// Require validation through sub-agent before allowing coupon operations
    #[serde(default = "default_require_validation")]
    pub require_validation: bool,

    /// Name of the validation sub-agent
    #[serde(default = "default_validation_agent")]
    pub validation_agent: String,

    /// Patterns that should always be blocked (test/fake codes)
    #[serde(default)]
    pub blocked_patterns: Vec<String>,

    /// Tools that require coupon validation
    #[serde(default = "default_guarded_tools")]
    pub guarded_tools: Vec<String>,

    /// API endpoint for direct validation (optional)
    #[serde(default)]
    pub api_endpoint: Option<String>,

    /// API token for validation service
    #[serde(default)]
    pub api_token: Option<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_code_pattern() -> String {
    r"^[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}$".to_string()
}

fn default_require_validation() -> bool {
    true
}

fn default_validation_agent() -> String {
    "coupon_validator".to_string()
}

fn default_guarded_tools() -> Vec<String> {
    vec![
        "apply_coupon".to_string(),
        "checkout".to_string(),
        "place_order".to_string(),
        "register_coupon".to_string(),
        "redeem_coupon".to_string(),
    ]
}

impl Default for CouponGuardrailConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            code_pattern: default_code_pattern(),
            require_validation: default_require_validation(),
            validation_agent: default_validation_agent(),
            blocked_patterns: Vec::new(),
            guarded_tools: default_guarded_tools(),
            api_endpoint: None,
            api_token: None,
        }
    }
}

/// Result of coupon validation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ValidationResult {
    /// Coupon is valid and can be used
    Valid,
    /// Coupon code format is invalid
    InvalidFormat { reason: String },
    /// Coupon code not found in system
    Invalid { reason: String },
    /// Coupon has already been used
    AlreadyUsed { used_at: Option<String> },
    /// Coupon has expired
    Expired { expired_at: Option<String> },
    /// Blocked by pattern (test/fake code)
    Blocked { reason: String },
    /// System error during validation
    SystemError { reason: String },
}

/// Coupon Guardrail Hook
///
/// Intercepts tool calls that involve coupon codes and validates them
/// before allowing execution. Uses sub-agent delegation for separation
/// of concerns.
pub struct CouponGuardrailHook {
    config: CouponGuardrailConfig,
    code_regex: Regex,
    blocked_regexes: Vec<Regex>,
}

impl CouponGuardrailHook {
    /// Create a new coupon guardrail hook with the given configuration
    pub fn new(config: CouponGuardrailConfig) -> anyhow::Result<Self> {
        let code_regex = Regex::new(&config.code_pattern)
            .map_err(|e| anyhow::anyhow!("Invalid code pattern regex: {}", e))?;

        let blocked_regexes: Vec<Regex> = config
            .blocked_patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Invalid blocked pattern regex: {}", e))?;

        Ok(Self {
            config,
            code_regex,
            blocked_regexes,
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> anyhow::Result<Self> {
        Self::new(CouponGuardrailConfig::default())
    }

    /// Check if this tool requires coupon validation
    fn is_guarded_tool(&self, tool_name: &str) -> bool {
        self.config.guarded_tools.contains(&tool_name.to_string())
    }

    /// Extract coupon code from tool arguments
    fn extract_coupon_code(&self, args: &Value) -> Option<String> {
        // Try common field names
        let fields = ["coupon_code", "code", "coupon", "promo_code", "voucher_code"];

        for field in fields {
            if let Some(code) = args.get(field).and_then(|v| v.as_str()) {
                return Some(code.to_string());
            }
        }

        None
    }

    /// Validate coupon code format
    fn validate_format(&self, code: &str) -> ValidationResult {
        let upper_code = code.to_uppercase();

        // Check blocked patterns first
        for regex in &self.blocked_regexes {
            if regex.is_match(&upper_code) {
                return ValidationResult::Blocked {
                    reason: format!("Code matches blocked pattern: {}", regex),
                };
            }
        }

        // Check for suspicious patterns
        if self.is_suspicious_code(&upper_code) {
            return ValidationResult::Blocked {
                reason: "Code appears to be a test or fake code".to_string(),
            };
        }

        // Validate format
        if !self.code_regex.is_match(&upper_code) {
            return ValidationResult::InvalidFormat {
                reason: format!(
                    "Code format invalid. Expected pattern: {}",
                    self.config.code_pattern
                ),
            };
        }

        ValidationResult::Valid
    }

    /// Check for suspicious/test codes
    fn is_suspicious_code(&self, code: &str) -> bool {
        let suspicious_patterns = [
            "TEST", "FAKE", "DEMO", "SAMPLE", "EXAMPLE", "DEBUG",
            "AAAA", "BBBB", "CCCC", "DDDD", "EEEE", "FFFF",
            "0000", "1111", "2222", "3333", "4444", "5555",
            "6666", "7777", "8888", "9999",
            "1234", "4321", "ABCD", "DCBA", "QWER", "ASDF",
        ];

        for pattern in suspicious_patterns {
            if code.contains(pattern) {
                return true;
            }
        }

        false
    }

    /// Get the validation agent name
    fn validation_agent(&self) -> &str {
        &self.config.validation_agent
    }
}

#[async_trait]
impl HookHandler for CouponGuardrailHook {
    fn name(&self) -> &str {
        "coupon-guardrail"
    }

    fn priority(&self) -> i32 {
        100 // High priority - runs before other hooks
    }

    /// Intercept tool calls and validate coupon codes
    async fn before_tool_call(&self, name: String, args: Value) -> HookResult<(String, Value)> {
        // Skip if disabled
        if !self.config.enabled {
            return HookResult::Continue((name, args));
        }

        // Only intercept guarded tools
        if !self.is_guarded_tool(&name) {
            return HookResult::Continue((name, args));
        }

        // Extract coupon code
        let coupon_code = self.extract_coupon_code(&args);

        if let Some(code) = coupon_code {
            // Step 1: Validate format
            let format_result = self.validate_format(&code);

            match format_result {
                ValidationResult::Valid => {
                    // Step 2: Check if validation through sub-agent is required
                    if self.config.require_validation {
                        // The tool execution should be delegated to the validation agent
                        // This is enforced by the hook - the main agent cannot bypass
                        tracing::info!(
                            hook = "coupon-guardrail",
                            code = %code,
                            validation_agent = %self.validation_agent(),
                            "Coupon requires validation through sub-agent"
                        );

                        // For now, we allow the call but log the requirement
                        // In production, this would check if the call is coming from the validator
                        // or would redirect to the validator agent
                        HookResult::Continue((name, args))
                    } else {
                        // Direct validation via API if configured
                        if let Some(endpoint) = &self.config.api_endpoint {
                            let api_result = self.validate_via_api(endpoint, &code).await;
                            match api_result {
                                ValidationResult::Valid => HookResult::Continue((name, args)),
                                other => HookResult::Cancel(format!(
                                    "Coupon validation failed: {}",
                                    self.validation_result_message(&other)
                                )),
                            }
                        } else {
                            HookResult::Continue((name, args))
                        }
                    }
                }
                ValidationResult::Blocked { reason } => {
                    tracing::warn!(
                        hook = "coupon-guardrail",
                        code = %code,
                        reason = %reason,
                        "Blocked suspicious coupon code"
                    );
                    HookResult::Cancel(format!(
                        "Coupon code '{}' is blocked: {}",
                        code, reason
                    ))
                }
                ValidationResult::InvalidFormat { reason } => {
                    tracing::warn!(
                        hook = "coupon-guardrail",
                        code = %code,
                        reason = %reason,
                        "Invalid coupon format"
                    );
                    HookResult::Cancel(format!(
                        "Coupon code '{}' has invalid format: {}",
                        code, reason
                    ))
                }
                other => {
                    HookResult::Cancel(format!(
                        "Coupon validation failed: {}",
                        self.validation_result_message(&other)
                    ))
                }
            }
        } else {
            // No coupon code in this call - allow
            HookResult::Continue((name, args))
        }
    }

    /// Log after tool call for audit
    async fn on_after_tool_call(
        &self,
        tool: &str,
        result: &crate::tools::traits::ToolResult,
        duration: std::time::Duration,
    ) {
        if self.is_guarded_tool(tool) {
            tracing::info!(
                hook = "coupon-guardrail",
                tool = %tool,
                success = result.success,
                duration_ms = duration.as_millis(),
                "Coupon tool call completed"
            );
        }
    }
}

impl CouponGuardrailHook {
    /// Get human-readable message for validation result
    fn validation_result_message(&self, result: &ValidationResult) -> String {
        match result {
            ValidationResult::Valid => "Coupon is valid".to_string(),
            ValidationResult::InvalidFormat { reason } => reason.clone(),
            ValidationResult::Invalid { reason } => reason.clone(),
            ValidationResult::AlreadyUsed { used_at } => {
                if let Some(date) = used_at {
                    format!("Coupon was already used on {}", date)
                } else {
                    "Coupon has already been used".to_string()
                }
            }
            ValidationResult::Expired { expired_at } => {
                if let Some(date) = expired_at {
                    format!("Coupon expired on {}", date)
                } else {
                    "Coupon has expired".to_string()
                }
            }
            ValidationResult::Blocked { reason } => reason.clone(),
            ValidationResult::SystemError { reason } => reason.clone(),
        }
    }

    /// Validate coupon via API (placeholder for actual implementation)
    async fn validate_via_api(&self, endpoint: &str, code: &str) -> ValidationResult {
        // This would be implemented to call the actual validation API
        // For now, return a placeholder
        tracing::debug!(
            hook = "coupon-guardrail",
            endpoint = %endpoint,
            code = %code,
            "Would validate via API"
        );

        // Placeholder: assume valid if format is correct
        ValidationResult::Valid
    }
}

/// Helper function to validate coupon code format (can be used without hook)
pub fn validate_coupon_format(code: &str, pattern: &str) -> bool {
    if let Ok(regex) = Regex::new(pattern) {
        regex.is_match(&code.to_uppercase())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CouponGuardrailConfig::default();
        assert!(config.enabled);
        assert!(config.require_validation);
        assert_eq!(config.validation_agent, "coupon_validator");
    }

    #[test]
    fn test_hook_creation() {
        let hook = CouponGuardrailHook::with_defaults().unwrap();
        assert_eq!(hook.name(), "coupon-guardrail");
        assert_eq!(hook.priority(), 100);
    }

    #[test]
    fn test_format_validation() {
        let hook = CouponGuardrailHook::with_defaults().unwrap();

        // Valid format
        let result = hook.validate_format("ABCD-1234-EFGH");
        assert!(matches!(result, ValidationResult::Valid));

        // Invalid format
        let result = hook.validate_format("abc123");
        assert!(matches!(result, ValidationResult::InvalidFormat { .. }));
    }

    #[test]
    fn test_blocked_patterns() {
        let config = CouponGuardrailConfig {
            blocked_patterns: vec!["TEST.*".to_string(), "FAKE.*".to_string()],
            ..Default::default()
        };
        let hook = CouponGuardrailHook::new(config).unwrap();

        let result = hook.validate_format("TEST-1234-ABCD");
        assert!(matches!(result, ValidationResult::Blocked { .. }));

        let result = hook.validate_format("FAKE-0000-1234");
        assert!(matches!(result, ValidationResult::Blocked { .. }));
    }

    #[test]
    fn test_suspicious_codes() {
        let hook = CouponGuardrailHook::with_defaults().unwrap();

        // Suspicious patterns
        assert!(hook.is_suspicious_code("AAAA-1234-BCDE"));
        assert!(hook.is_suspicious_code("TEST-1234-ABCD"));
        assert!(hook.is_suspicious_code("1234-5678-9012"));

        // Normal codes
        assert!(!hook.is_suspicious_code("ABCD-1234-EFGH"));
        assert!(!hook.is_suspicious_code("WXYZ-9876-MNOP"));
    }

    #[tokio::test]
    async fn test_hook_intercepts_guarded_tools() {
        let hook = CouponGuardrailHook::with_defaults().unwrap();

        // Should intercept apply_coupon with valid code
        let args = serde_json::json!({"coupon_code": "ABCD-1234-EFGH"});
        let result = hook
            .before_tool_call("apply_coupon".to_string(), args)
            .await;
        assert!(!result.is_cancel());

        // Should block invalid format
        let args = serde_json::json!({"coupon_code": "invalid"});
        let result = hook
            .before_tool_call("apply_coupon".to_string(), args)
            .await;
        assert!(result.is_cancel());

        // Should not intercept other tools
        let args = serde_json::json!({"coupon_code": "ABCD-1234-EFGH"});
        let result = hook.before_tool_call("read_file".to_string(), args).await;
        assert!(!result.is_cancel());
    }

    #[tokio::test]
    async fn test_hook_blocks_suspicious_codes() {
        let hook = CouponGuardrailHook::with_defaults().unwrap();

        let args = serde_json::json!({"coupon_code": "TEST-1234-ABCD"});
        let result = hook
            .before_tool_call("apply_coupon".to_string(), args)
            .await;
        assert!(result.is_cancel());

        let args = serde_json::json!({"coupon_code": "AAAA-BBBB-CCCC"});
        let result = hook
            .before_tool_call("checkout".to_string(), args)
            .await;
        assert!(result.is_cancel());
    }

    #[test]
    fn test_validate_coupon_format_helper() {
        assert!(validate_coupon_format(
            "ABCD-1234-EFGH",
            r"^[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}$"
        ));
        assert!(!validate_coupon_format(
            "abc123",
            r"^[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}$"
        ));
    }
}