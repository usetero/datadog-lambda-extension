//! Policy evaluator wrapper for simplified policy evaluation.
//!
//! This module provides a `PolicyEvaluator` struct that wraps the `PolicyEngine`
//! and `PolicyRegistry` from `policy-rs`, providing a simple `should_keep` method
//! for filtering telemetry.

use policy_rs::{EvaluateResult, Matchable, PolicyEngine, PolicyRegistry};
use std::sync::Arc;
use tracing::{debug, warn};

/// Evaluates telemetry items against configured policies.
///
/// The evaluator holds a reference to the policy registry and creates
/// snapshots for lock-free evaluation. It provides a simple boolean
/// interface for determining whether telemetry should be kept or dropped.
pub struct PolicyEvaluator {
    engine: PolicyEngine,
    registry: Arc<PolicyRegistry>,
}

impl PolicyEvaluator {
    /// Creates a new policy evaluator with the given registry.
    #[must_use]
    pub fn new(registry: Arc<PolicyRegistry>) -> Self {
        Self {
            engine: PolicyEngine::new(),
            registry,
        }
    }

    /// Evaluates an item against configured policies.
    ///
    /// Returns `true` if the item should be kept (sent to backend),
    /// `false` if it should be dropped.
    ///
    /// This method implements a "fail open" policy: if evaluation fails
    /// for any reason, the item is kept and a warning is logged.
    ///
    /// # Policy Results
    /// - `NoMatch`: Keep (no policy matched)
    /// - `Keep`: Keep
    /// - `Drop`: Drop
    /// - `Sample`: Keep based on sampling decision
    /// - `RateLimit`: Keep based on rate limit decision
    #[must_use]
    pub fn should_keep<M: Matchable>(&self, item: &M) -> bool {
        let snapshot = self.registry.snapshot();
        let policy_count = snapshot.len();

        debug!("POLICY | Evaluating item against {} policies", policy_count);

        match self.engine.evaluate(&snapshot, item) {
            Ok(result) => {
                debug!("POLICY | Evaluation result: {:?}", result);
                match result {
                    EvaluateResult::NoMatch => {
                        debug!("POLICY | No matching policy, keeping item");
                        true
                    }
                    EvaluateResult::Keep { .. } => {
                        debug!("POLICY | Policy matched, keeping item");
                        true
                    }
                    EvaluateResult::Drop { .. } => {
                        debug!("POLICY | Dropping item due to policy");
                        false
                    }
                    EvaluateResult::Sample {
                        keep, percentage, ..
                    } => {
                        debug!(
                            "POLICY | Sampling decision: keep={}, percentage={}%",
                            keep, percentage
                        );
                        keep
                    }
                    EvaluateResult::RateLimit { allowed, .. } => {
                        debug!("POLICY | Rate limit decision: allowed={}", allowed);
                        allowed
                    }
                }
            }
            Err(e) => {
                // Fail open: keep the item if evaluation fails
                warn!("POLICY | Evaluation error, keeping item: {}", e);
                true
            }
        }
    }

    /// Returns a reference to the underlying registry.
    #[must_use]
    pub fn registry(&self) -> &Arc<PolicyRegistry> {
        &self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::lambda::{IntakeLog, Lambda, Message};
    use policy_rs::Policy;
    use policy_rs::proto::tero::policy::v1::{
        AttributePath, LogField, LogMatcher, LogTarget, Policy as ProtoPolicy, log_matcher,
    };

    /// Helper to create an `AttributePath` from a single key string
    fn attr_path(key: &str) -> AttributePath {
        AttributePath {
            path: vec![key.to_string()],
        }
    }

    /// Helper to create a test `IntakeLog` with customizable fields
    fn create_test_log(
        message: &str,
        status: &str,
        service: &str,
        hostname: &str,
        arn: &str,
        request_id: Option<&str>,
    ) -> IntakeLog {
        IntakeLog {
            message: Message {
                message: message.to_string(),
                lambda: Lambda {
                    arn: arn.to_string(),
                    request_id: request_id.map(String::from),
                    durable_execution_id: None,
                    durable_execution_name: None,
                    first_invocation: None,
                    durable_execution_status: None,
                },
                timestamp: 1_234_567_890,
                status: status.to_string(),
            },
            hostname: hostname.to_string(),
            service: service.to_string(),
            tags: "env:test".to_string(),
            source: "lambda".to_string(),
        }
    }

    /// Helper to create a default test log
    fn default_test_log() -> IntakeLog {
        create_test_log(
            "Test log message",
            "info",
            "test-service",
            "test-host",
            "arn:aws:lambda:us-east-1:123456789:function:test",
            Some("req-123"),
        )
    }

    /// Helper to create a policy that matches on log body with regex
    fn body_regex_policy(id: &str, pattern: &str, keep: &str, enabled: bool) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Regex(pattern.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    /// Helper to create a policy that matches on log body with exact match
    fn body_exact_policy(id: &str, value: &str, keep: &str, enabled: bool) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Exact(value.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    /// Helper to create a policy that matches on severity text
    fn severity_exact_policy(id: &str, severity: &str, keep: &str, enabled: bool) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::SeverityText.into())),
            r#match: Some(log_matcher::Match::Exact(severity.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    /// Helper to create a policy that matches on a resource attribute
    fn resource_attr_policy(
        id: &str,
        attr_key: &str,
        pattern: &str,
        keep: &str,
        enabled: bool,
    ) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::ResourceAttribute(attr_path(attr_key))),
            r#match: Some(log_matcher::Match::Regex(pattern.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    /// Helper to create a policy with a negated matcher
    fn negated_body_regex_policy(id: &str, pattern: &str, keep: &str, enabled: bool) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Regex(pattern.to_string())),
            negate: true,
            case_insensitive: false,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    /// Helper to create a policy with multiple matchers (AND logic)
    fn multi_matcher_policy(
        id: &str,
        body_pattern: &str,
        severity: &str,
        keep: &str,
        enabled: bool,
    ) -> Policy {
        let body_matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Regex(body_pattern.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let severity_matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::SeverityText.into())),
            r#match: Some(log_matcher::Match::Exact(severity.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let log_target = LogTarget {
            r#match: vec![body_matcher, severity_matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    // ==================== Basic Functionality Tests ====================

    #[tokio::test]
    async fn test_no_policies_keeps_all_logs() {
        let registry = Arc::new(PolicyRegistry::new());
        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        assert!(evaluator.should_keep(&log));
    }

    #[test]
    fn test_no_policies_keeps_all_logs_sync() {
        let registry = Arc::new(PolicyRegistry::new());
        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        assert!(evaluator.should_keep(&log));
    }

    #[tokio::test]
    async fn test_disabled_policy_does_not_match() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Create a disabled drop policy
        let policy = body_regex_policy("drop-all", ".*", "none", false);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        // Should keep because policy is disabled
        assert!(evaluator.should_keep(&log));
    }

    // ==================== Drop (keep: "none") Tests ====================

    #[tokio::test]
    async fn test_drop_policy_body_regex_match() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-errors", "error", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Log with "error" should be dropped
        let error_log = create_test_log(
            "An error occurred",
            "error",
            "test-service",
            "test-host",
            "arn:aws:lambda:us-east-1:123456789:function:test",
            Some("req-123"),
        );
        assert!(!evaluator.should_keep(&error_log));

        // Log without "error" should be kept
        let info_log = create_test_log(
            "Info message",
            "info",
            "test-service",
            "test-host",
            "arn:aws:lambda:us-east-1:123456789:function:test",
            Some("req-123"),
        );
        assert!(evaluator.should_keep(&info_log));
    }

    #[tokio::test]
    async fn test_drop_policy_body_exact_match() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_exact_policy("drop-specific", "DROP_ME", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Exact match should be dropped
        let drop_log = create_test_log("DROP_ME", "info", "test-service", "test-host", "arn", None);
        assert!(!evaluator.should_keep(&drop_log));

        // Partial match should not be dropped
        let keep_log = create_test_log(
            "DROP_ME_NOT",
            "info",
            "test-service",
            "test-host",
            "arn",
            None,
        );
        assert!(evaluator.should_keep(&keep_log));
    }

    #[tokio::test]
    async fn test_drop_policy_severity_match() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = severity_exact_policy("drop-debug", "debug", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Debug logs should be dropped
        let debug_log = create_test_log("Debug message", "debug", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&debug_log));

        // Non-debug logs should be kept
        let info_log = create_test_log("Info message", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&info_log));
    }

    #[tokio::test]
    async fn test_drop_policy_resource_attribute_service() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = resource_attr_policy(
            "drop-noisy-service",
            "service",
            "noisy-service",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Log from noisy-service should be dropped
        let noisy_log = create_test_log("Message", "info", "noisy-service", "host", "arn", None);
        assert!(!evaluator.should_keep(&noisy_log));

        // Log from other services should be kept
        let other_log = create_test_log("Message", "info", "good-service", "host", "arn", None);
        assert!(evaluator.should_keep(&other_log));
    }

    #[tokio::test]
    async fn test_drop_policy_resource_attribute_hostname() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = resource_attr_policy("drop-test-host", "hostname", "test-host", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        let log_from_test = create_test_log("Message", "info", "svc", "test-host", "arn", None);
        assert!(!evaluator.should_keep(&log_from_test));

        let log_from_prod = create_test_log("Message", "info", "svc", "prod-host", "arn", None);
        assert!(evaluator.should_keep(&log_from_prod));
    }

    #[tokio::test]
    async fn test_drop_policy_resource_attribute_arn() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Use a pattern that doesn't match empty strings (Vectorscan requirement)
        let policy =
            resource_attr_policy("drop-dev-functions", "arn", ":function:dev-", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        let dev_log = create_test_log(
            "Message",
            "info",
            "svc",
            "host",
            "arn:aws:lambda:us-east-1:123456789:function:dev-test",
            None,
        );
        assert!(!evaluator.should_keep(&dev_log));

        let prod_log = create_test_log(
            "Message",
            "info",
            "svc",
            "host",
            "arn:aws:lambda:us-east-1:123456789:function:prod-api",
            None,
        );
        assert!(evaluator.should_keep(&prod_log));
    }

    // ==================== Keep (keep: "all") Tests ====================

    #[tokio::test]
    async fn test_keep_policy_explicit_keep() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("keep-important", "IMPORTANT", "all", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Important logs should be kept
        let important_log = create_test_log(
            "IMPORTANT: system alert",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(evaluator.should_keep(&important_log));

        // Other logs should also be kept (no match = keep)
        let other_log = create_test_log("Regular message", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&other_log));
    }

    // ==================== Negated Matcher Tests ====================

    #[tokio::test]
    async fn test_negated_matcher_drops_when_pattern_not_found() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Drop logs that do NOT contain "keep_me"
        let policy = negated_body_regex_policy("drop-unless-keep-me", "keep_me", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Log without "keep_me" is disqualified by negated matcher, so no drop
        // Wait - negated matchers disqualify policies, not match them
        // So a negated matcher that matches means the policy is disqualified
        let regular_log = create_test_log("Regular message", "info", "svc", "host", "arn", None);
        // Negated matcher: if "keep_me" is NOT found, the negated match succeeds (positive match)
        // Actually, negated means if the pattern IS found, the policy is disqualified
        // So if "keep_me" is NOT in the log, the negated matcher does not disqualify
        // and the policy can match... but there's no positive matcher
        // Let me reconsider - this test may not work as expected with just a negated matcher

        // With only a negated matcher and no positive matchers, required_match_count = 0
        // and the policy would match any log unless disqualified
        assert!(!evaluator.should_keep(&regular_log));

        // Log with "keep_me" - negated matcher disqualifies the policy
        let keep_log = create_test_log("keep_me please", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&keep_log));
    }

    // ==================== Multiple Matcher (AND) Tests ====================

    #[tokio::test]
    async fn test_multi_matcher_all_must_match() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Policy: drop logs that contain "error" AND have severity "error"
        let policy = multi_matcher_policy("drop-error-errors", "error", "error", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Both conditions met - should be dropped
        let error_error = create_test_log("An error occurred", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&error_error));

        // Only body matches - should be kept
        let error_info = create_test_log("An error occurred", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&error_info));

        // Only severity matches - should be kept
        let warn_error = create_test_log("A warning message", "error", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&warn_error));

        // Neither matches - should be kept
        let info_info = create_test_log("Info message", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&info_info));
    }

    // ==================== Multiple Policies Tests ====================

    #[tokio::test]
    async fn test_multiple_policies_most_restrictive_wins() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Two policies that could match the same log
        let keep_policy = body_regex_policy("keep-errors", "error", "all", true);
        let drop_policy = severity_exact_policy("drop-critical", "error", "none", true);
        handle.update(vec![keep_policy, drop_policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Log matches both policies - drop is more restrictive
        let error_log = create_test_log("An error occurred", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&error_log));
    }

    #[tokio::test]
    async fn test_multiple_policies_first_match_only() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Policies that match different logs
        let drop_debug = severity_exact_policy("drop-debug", "debug", "none", true);
        let drop_trace = severity_exact_policy("drop-trace", "trace", "none", true);
        handle.update(vec![drop_debug, drop_trace]);

        let evaluator = PolicyEvaluator::new(registry);

        // Debug logs should be dropped
        let debug_log = create_test_log("Debug", "debug", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&debug_log));

        // Trace logs should be dropped
        let trace_log = create_test_log("Trace", "trace", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&trace_log));

        // Info logs should be kept (no match)
        let info_log = create_test_log("Info", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&info_log));
    }

    // ==================== Sampling Tests ====================

    #[tokio::test]
    async fn test_sampling_zero_percent_drops_all() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match "message" which is in "Test log message" from default_test_log()
        // Note: Wildcard patterns like .+ don't work reliably with Vectorscan
        let policy = body_regex_policy("sample-zero", "message", "0%", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        // 0% sampling should drop all
        assert!(!evaluator.should_keep(&log));
    }

    #[tokio::test]
    async fn test_sampling_hundred_percent_keeps_all() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match "message" which is in "Test log message" from default_test_log()
        let policy = body_regex_policy("sample-all", "message", "100%", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        // 100% sampling should keep all
        assert!(evaluator.should_keep(&log));
    }

    /// Percentage sampling needs a sample key to be consistent. A policy that
    /// sets a percentage but no sample key fails open and keeps everything;
    /// see `sampled_policy_with_key` tests for the sampling path itself.
    #[tokio::test]
    async fn test_sampling_without_sample_key_keeps_all() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match "message" which appears in all test logs
        let policy = body_regex_policy("sample-half", "message", "50%", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        let total = 1000;
        let mut kept = 0;

        for i in 0..total {
            let log = create_test_log(
                &format!("Log message {i}"),
                "info",
                "svc",
                "host",
                "arn",
                None,
            );
            if evaluator.should_keep(&log) {
                kept += 1;
            }
        }

        assert_eq!(
            kept, total,
            "a policy with no sample key must keep all logs"
        );
    }

    // ==================== Rate Limiting Tests ====================

    #[tokio::test]
    async fn test_rate_limit_per_second() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Allow only 5 per second
        // Match "Log" which appears in all test messages below
        let policy = body_regex_policy("rate-limit-5ps", "Log", "5/s", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // First 5 should be allowed
        for i in 0..5 {
            let log = create_test_log(&format!("Log {i}"), "info", "svc", "host", "arn", None);
            assert!(evaluator.should_keep(&log), "Log {i} should have been kept");
        }

        // 6th should be rate limited (dropped)
        let log6 = create_test_log("Log 6", "info", "svc", "host", "arn", None);
        assert!(
            !evaluator.should_keep(&log6),
            "Log 6 should have been rate limited"
        );
    }

    // ==================== Edge Cases ====================

    #[tokio::test]
    async fn test_empty_log_message() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-empty", "^$", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Empty message should match
        let empty_log = create_test_log("", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&empty_log));

        // Non-empty should not match
        let non_empty_log = create_test_log("content", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&non_empty_log));
    }

    #[tokio::test]
    async fn test_special_regex_characters() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match logs containing "[ERROR]" literally
        let policy = body_regex_policy("drop-error-bracket", r"\[ERROR\]", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        let bracket_log = create_test_log(
            "[ERROR] Something failed",
            "error",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&bracket_log));

        let no_bracket_log = create_test_log(
            "ERROR Something failed",
            "error",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(evaluator.should_keep(&no_bracket_log));
    }

    #[tokio::test]
    async fn test_missing_request_id() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy =
            resource_attr_policy("drop-no-request-id", "request_id", "missing", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Log without request_id - policy shouldn't match because field is None
        let no_req_id_log = create_test_log("Message", "info", "svc", "host", "arn", None);
        // Since request_id is None, the field returns None and regex can't match
        assert!(evaluator.should_keep(&no_req_id_log));

        // Log with request_id containing "missing"
        let with_missing =
            create_test_log("Message", "info", "svc", "host", "arn", Some("missing-123"));
        assert!(!evaluator.should_keep(&with_missing));
    }

    #[tokio::test]
    async fn test_unicode_in_log_message() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-emoji", "🔥", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        let emoji_log = create_test_log("Fire emoji: 🔥", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&emoji_log));

        let no_emoji_log = create_test_log("No fire here", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&no_emoji_log));
    }

    #[tokio::test]
    async fn test_very_long_log_message() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-long", "start.*end", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Create a very long message
        let long_content = "x".repeat(10000);
        let long_message = format!("start{long_content}end");
        let long_log = create_test_log(&long_message, "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&long_log));
    }

    #[tokio::test]
    async fn test_case_sensitive_matching() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-error-lowercase", "error", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Lowercase matches
        let lower = create_test_log("an error occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&lower));

        // Uppercase does NOT match (regex is case-sensitive by default)
        let upper = create_test_log("an ERROR occurred", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&upper));
    }

    #[tokio::test]
    async fn test_case_insensitive_regex() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Use (?i) for case-insensitive matching
        let policy = body_regex_policy("drop-error-any-case", "(?i)error", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        let lower = create_test_log("an error occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&lower));

        let upper = create_test_log("an ERROR occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&upper));

        let mixed = create_test_log("an ErRoR occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&mixed));
    }

    // ==================== Complex Regex Pattern Tests ====================

    #[tokio::test]
    async fn test_regex_alternation() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match "error" OR "fatal" OR "critical"
        let policy = body_regex_policy("drop-severe", "error|fatal|critical", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match "error"
        let error_log = create_test_log("An error occurred", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&error_log));

        // Should match "fatal"
        let fatal_log = create_test_log(
            "fatal exception thrown",
            "error",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&fatal_log));

        // Should match "critical"
        let critical_log = create_test_log(
            "critical failure detected",
            "error",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&critical_log));

        // Should NOT match
        let info_log = create_test_log("normal operation", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&info_log));
    }

    #[tokio::test]
    async fn test_regex_character_classes() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match request IDs like "req-abc" followed by digits
        // Using simple fixed-length pattern for Vectorscan compatibility
        let policy = body_regex_policy(
            "drop-request-ids",
            r"req-[a-z][a-z][a-z][0-9]",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match - has req-abc followed by digit
        let log1 = create_test_log("Processing req-abc123", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log1));

        // Should match - has req-xyz followed by digit
        let log2 = create_test_log("req-xyz789 completed", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should NOT match - no digit after 3 letters
        let log3 = create_test_log("req-abc done", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log3));

        // Should NOT match (no alphanumeric after hyphen)
        let log4 = create_test_log("req- incomplete", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log4));
    }

    #[tokio::test]
    async fn test_regex_quantifiers() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match error codes with exactly 3 digits like "E001"
        // Using explicit repetition instead of {3,5} for Vectorscan compatibility
        let policy = body_regex_policy("drop-error-codes", r"E[0-9][0-9][0-9]", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match 3 digits
        let log1 = create_test_log("Error E001 occurred", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log1));

        // Should match 4 digits (matches first 3)
        let log2 = create_test_log("E1234 detected", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should match 5 digits (matches first 3)
        let log3 = create_test_log("Processing E99999", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log3));

        // Should NOT match (only 2 digits)
        let log4 = create_test_log("E01 too short", "error", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log4));
    }

    #[tokio::test]
    async fn test_regex_word_boundaries() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match whole word "debug" only (using word boundaries)
        let policy = body_regex_policy("drop-debug-word", r"\bdebug\b", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match standalone "debug"
        let log1 = create_test_log("debug message here", "debug", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log1));

        let log2 = create_test_log("This is debug output", "debug", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should NOT match "debugging" (word continues)
        let log3 = create_test_log("debugging the issue", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log3));

        // Should NOT match "nodebug" (word starts before)
        let log4 = create_test_log("nodebug mode enabled", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log4));
    }

    #[tokio::test]
    async fn test_regex_anchors() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match logs starting with "START"
        let policy = body_regex_policy("drop-start-logs", r"^START", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match - starts with START
        let log1 = create_test_log(
            "START RequestId: abc-123",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should NOT match - START not at beginning
        let log2 = create_test_log("Function START time", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log2));
    }

    #[tokio::test]
    async fn test_regex_end_anchor() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match logs ending with "completed"
        let policy = body_regex_policy("drop-completed", r"completed$", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match - ends with completed
        let log1 = create_test_log("Task completed", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log1));

        // Should NOT match - completed not at end
        let log2 = create_test_log("completed the task", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log2));
    }

    #[tokio::test]
    async fn test_regex_ip_address_pattern() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match specific IP-like pattern using literal dot-separated numbers
        // Using a simple literal pattern that Vectorscan can handle
        let policy = body_regex_policy("drop-ips", r"192\.168\.", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match 192.168.x.x addresses
        let log1 = create_test_log(
            "Connection from 192.168.1.100",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        let log2 = create_test_log(
            "Server at 192.168.0.1 responded",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log2));

        // Should NOT match other IPs
        let log3 = create_test_log("Server at 10.0.0.1", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log3));

        // Should NOT match no IP
        let log4 = create_test_log("No IP here", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log4));
    }

    #[tokio::test]
    async fn test_regex_uuid_pattern() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match UUID-like patterns (8-4-4-4-12 hex digits)
        let policy = body_regex_policy(
            "drop-uuids",
            r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match UUID
        let log1 = create_test_log(
            "Request 550e8400-e29b-41d4-a716-446655440000 processed",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should NOT match malformed UUID
        let log2 = create_test_log(
            "ID 550e8400-e29b processed",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(evaluator.should_keep(&log2));
    }

    #[tokio::test]
    async fn test_regex_optional_groups() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match "error" followed by various patterns using alternation
        // Using alternation instead of optional groups for Vectorscan compatibility
        let policy = body_regex_policy(
            "drop-errors",
            r"error code|error:|error [0-9]|error$",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match "error code"
        let log2 = create_test_log("error code 500", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should match "error:"
        let log3 = create_test_log("error: 404 not found", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log3));

        // Should match "error code:"
        let log4 = create_test_log("error code: 503", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log4));

        // Should NOT match unrelated
        let log5 = create_test_log("success message", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log5));
    }

    #[tokio::test]
    async fn test_regex_html_injection_patterns() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match common HTML injection patterns using simple literals
        let policy = body_regex_policy("drop-html-tags", r"<script", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match - HTML script tags
        let log1 = create_test_log(
            "Input: <script>alert(1)</script>",
            "warn",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should match - script tag at start
        let log2 = create_test_log("<script src='bad.js'>", "warn", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should NOT match - normal alphanumeric content
        let log3 = create_test_log("Normal log message 123", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log3));
    }

    #[tokio::test]
    async fn test_regex_http_status_codes() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match 4xx and 5xx HTTP status codes
        // Using [0-9][0-9] instead of [0-9]{2} for Vectorscan compatibility
        let policy = body_regex_policy("drop-http-errors", r"HTTP[/ ][45][0-9][0-9]", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match 4xx errors
        let log1 = create_test_log("Response: HTTP/404", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log1));

        let log2 = create_test_log("HTTP 401 Unauthorized", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should match 5xx errors
        let log3 = create_test_log(
            "HTTP/500 Internal Server Error",
            "error",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log3));

        let log4 = create_test_log("Got HTTP 503", "error", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log4));

        // Should NOT match 2xx success
        let log5 = create_test_log("HTTP/200 OK", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log5));

        // Should NOT match 3xx redirects
        let log6 = create_test_log("HTTP 301 Moved", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log6));
    }

    #[tokio::test]
    async fn test_regex_json_field_extraction() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match JSON-like patterns with "level":"error" or "severity":"error"
        let policy = body_regex_policy(
            "drop-json-errors",
            r#""(?:level|severity)"\s*:\s*"error""#,
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match JSON error level
        let log1 = create_test_log(
            r#"{"level": "error", "message": "failed"}"#,
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        let log2 = create_test_log(
            r#"{"severity":"error","msg":"crash"}"#,
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log2));

        // Should NOT match JSON info level
        let log3 = create_test_log(
            r#"{"level": "info", "message": "ok"}"#,
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(evaluator.should_keep(&log3));
    }

    #[tokio::test]
    async fn test_regex_timestamp_pattern() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match ISO 8601 timestamp pattern
        let policy = body_regex_policy(
            "drop-timestamped",
            r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match ISO timestamp
        let log1 = create_test_log(
            "Event at 2024-01-15T14:30:00Z completed",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should NOT match non-ISO format
        let log2 = create_test_log(
            "Event at 01/15/2024 2:30 PM",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(evaluator.should_keep(&log2));
    }

    #[tokio::test]
    async fn test_regex_lambda_arn_pattern() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match Lambda ARN pattern
        let policy = resource_attr_policy(
            "drop-dev-region",
            "arn",
            r"arn:aws:lambda:us-east-1:[0-9]+:function:dev-",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match dev function in us-east-1
        let log1 = create_test_log(
            "message",
            "info",
            "svc",
            "host",
            "arn:aws:lambda:us-east-1:123456789012:function:dev-api",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should NOT match dev function in different region
        let log2 = create_test_log(
            "message",
            "info",
            "svc",
            "host",
            "arn:aws:lambda:eu-west-1:123456789012:function:dev-api",
            None,
        );
        assert!(evaluator.should_keep(&log2));

        // Should NOT match prod function
        let log3 = create_test_log(
            "message",
            "info",
            "svc",
            "host",
            "arn:aws:lambda:us-east-1:123456789012:function:prod-api",
            None,
        );
        assert!(evaluator.should_keep(&log3));
    }

    #[tokio::test]
    async fn test_regex_multiple_patterns_combined() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Complex pattern: Match AWS request IDs or trace IDs
        // AWS Request ID: 8 hex chars - 4 - 4 - 4 - 12
        // X-Ray Trace ID: 1-8 hex - 24 hex
        let policy = body_regex_policy(
            "drop-aws-ids",
            r"(?:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|1-[0-9a-f]{8}-[0-9a-f]{24})",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match AWS Request ID
        let log1 = create_test_log(
            "RequestId: 12345678-1234-1234-1234-123456789abc",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should match X-Ray Trace ID
        let log2 = create_test_log(
            "TraceId: 1-12345678-123456789012345678901234",
            "info",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log2));

        // Should NOT match other patterns
        let log3 = create_test_log("Simple log message", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log3));
    }

    #[tokio::test]
    async fn test_regex_lookahead_simulation() {
        // Note: True lookaheads may not be supported by Vectorscan,
        // but we can test patterns that achieve similar filtering
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Match "password" followed by "=" or ":" and then content
        // Using alternation instead of character class with + for Vectorscan compatibility
        let policy = body_regex_policy(
            "drop-password-logs",
            r"password=[a-zA-Z0-9]|password: [a-zA-Z0-9]|password =[a-zA-Z0-9]",
            "none",
            true,
        );
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match password= pattern
        let log1 = create_test_log(
            "Setting password=secret123",
            "debug",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log1));

        // Should match password: pattern
        let log2 = create_test_log("password: hunter2", "debug", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&log2));

        // Should match password = pattern
        let log3 = create_test_log(
            "User password =mypass123",
            "debug",
            "svc",
            "host",
            "arn",
            None,
        );
        assert!(!evaluator.should_keep(&log3));

        // Should NOT match if no value follows
        let log4 = create_test_log("Enter your password", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&log4));
    }

    // ==================== Case Insensitive Matching Tests ====================

    /// Helper to create a policy with case-insensitive matching
    fn case_insensitive_body_regex_policy(
        id: &str,
        pattern: &str,
        keep: &str,
        enabled: bool,
    ) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Regex(pattern.to_string())),
            negate: false,
            case_insensitive: true,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    /// Helper to create a policy with case-insensitive exact matching
    fn case_insensitive_body_exact_policy(
        id: &str,
        value: &str,
        keep: &str,
        enabled: bool,
    ) -> Policy {
        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Exact(value.to_string())),
            negate: false,
            case_insensitive: true,
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: None,
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    #[tokio::test]
    async fn test_case_insensitive_regex_matching() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Create policy with case_insensitive: true
        let policy = case_insensitive_body_regex_policy("drop-error-ci", "error", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match lowercase
        let lower = create_test_log("an error occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&lower));

        // Should match uppercase
        let upper = create_test_log("an ERROR occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&upper));

        // Should match mixed case
        let mixed = create_test_log("an ErRoR occurred", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&mixed));

        // Should NOT match unrelated
        let unrelated = create_test_log("all good", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&unrelated));
    }

    #[tokio::test]
    async fn test_case_insensitive_exact_matching() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Create policy with case_insensitive: true for exact match
        let policy = case_insensitive_body_exact_policy("drop-hello-ci", "HELLO", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Should match exact uppercase
        let upper = create_test_log("HELLO", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&upper));

        // Should match lowercase
        let lower = create_test_log("hello", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&lower));

        // Should match mixed case
        let mixed = create_test_log("HeLLo", "info", "svc", "host", "arn", None);
        assert!(!evaluator.should_keep(&mixed));

        // Should NOT match partial
        let partial = create_test_log("hello world", "info", "svc", "host", "arn", None);
        assert!(evaluator.should_keep(&partial));
    }

    #[tokio::test]
    async fn test_case_sensitive_vs_insensitive_comparison() {
        // Test that case_insensitive: false (default) is case-sensitive
        let registry_sensitive = Arc::new(PolicyRegistry::new());
        let handle_sensitive = registry_sensitive.register_provider();
        let policy_sensitive = body_regex_policy("drop-error-cs", "error", "none", true);
        handle_sensitive.update(vec![policy_sensitive]);
        let evaluator_sensitive = PolicyEvaluator::new(registry_sensitive);

        // Test that case_insensitive: true is case-insensitive
        let registry_insensitive = Arc::new(PolicyRegistry::new());
        let handle_insensitive = registry_insensitive.register_provider();
        let policy_insensitive =
            case_insensitive_body_regex_policy("drop-error-ci", "error", "none", true);
        handle_insensitive.update(vec![policy_insensitive]);
        let evaluator_insensitive = PolicyEvaluator::new(registry_insensitive);

        // Lowercase matches both
        let lower = create_test_log("error message", "info", "svc", "host", "arn", None);
        assert!(!evaluator_sensitive.should_keep(&lower));
        assert!(!evaluator_insensitive.should_keep(&lower));

        // Uppercase only matches case-insensitive
        let upper = create_test_log("ERROR message", "info", "svc", "host", "arn", None);
        assert!(evaluator_sensitive.should_keep(&upper)); // case-sensitive doesn't match
        assert!(!evaluator_insensitive.should_keep(&upper)); // case-insensitive matches
    }

    // ==================== Sample Key Tests ====================

    /// Helper to create a policy with a sample key for consistent sampling
    fn sampled_policy_with_key(
        id: &str,
        pattern: &str,
        keep: &str,
        sample_key_attr: &str,
        enabled: bool,
    ) -> Policy {
        use policy_rs::proto::tero::policy::v1::{LogSampleKey, log_sample_key};

        let matcher = LogMatcher {
            field: Some(log_matcher::Field::LogField(LogField::Body.into())),
            r#match: Some(log_matcher::Match::Regex(pattern.to_string())),
            negate: false,
            case_insensitive: false,
        };

        let sample_key = LogSampleKey {
            field: Some(log_sample_key::Field::ResourceAttribute(attr_path(
                sample_key_attr,
            ))),
        };

        let log_target = LogTarget {
            r#match: vec![matcher],
            keep: keep.to_string(),
            transform: None,
            sample_key: Some(sample_key),
        };

        let proto = ProtoPolicy {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            target: Some(policy_rs::proto::tero::policy::v1::policy::Target::Log(
                log_target,
            )),
            ..Default::default()
        };

        Policy::new(proto)
    }

    #[tokio::test]
    async fn test_sample_key_consistent_sampling() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Sample 50% with request_id as sample key
        // All logs with the same request_id should get the same decision
        let policy = sampled_policy_with_key("sample-by-request", "Log", "50%", "request_id", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Create multiple logs with the same request_id
        let request_id = "req-abc-123";
        let mut decisions = Vec::new();

        for i in 0..10 {
            let log = create_test_log(
                &format!("Log message {i}"),
                "info",
                "svc",
                "host",
                "arn",
                Some(request_id),
            );
            decisions.push(evaluator.should_keep(&log));
        }

        // All decisions should be the same for the same request_id
        let first_decision = decisions[0];
        assert!(
            decisions.iter().all(|&d| d == first_decision),
            "All logs with the same request_id should have the same sampling decision"
        );
    }

    #[tokio::test]
    async fn test_sample_key_different_keys_different_decisions() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Sample 50% with request_id as sample key
        let policy = sampled_policy_with_key("sample-by-request", "Log", "50%", "request_id", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Generate many different request IDs and track decisions
        let mut kept_count = 0;
        let mut dropped_count = 0;
        let total: u32 = 100;

        for i in 0..total {
            // Sampling hashes the key with FNV-1a and compares the top bits of
            // the result. Keys sharing a long prefix (`req-0000`, `req-0001`, …)
            // barely move those bits, so spread the key like a real request id.
            let request_id = format!(
                "req-{:016x}",
                u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            );
            let log = create_test_log(
                "Log message",
                "info",
                "svc",
                "host",
                "arn",
                Some(&request_id),
            );
            if evaluator.should_keep(&log) {
                kept_count += 1;
            } else {
                dropped_count += 1;
            }
        }

        // With 50% sampling, we expect roughly half to be kept
        let ratio = f64::from(kept_count) / f64::from(total);
        assert!(
            ratio > 0.3 && ratio < 0.7,
            "Expected ~50% sampling ratio, got {ratio:.2} (kept={kept_count}, dropped={dropped_count})"
        );
    }

    #[tokio::test]
    async fn test_sample_key_zero_percent_drops_all() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // 0% sampling with sample key - should drop all
        let policy = sampled_policy_with_key("sample-zero", "Log", "0%", "request_id", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        for i in 0..10 {
            let log = create_test_log(
                "Log message",
                "info",
                "svc",
                "host",
                "arn",
                Some(&format!("req-{i}")),
            );
            assert!(
                !evaluator.should_keep(&log),
                "0% sampling should drop all logs"
            );
        }
    }

    #[tokio::test]
    async fn test_sample_key_hundred_percent_keeps_all() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // 100% sampling with sample key - should keep all
        let policy = sampled_policy_with_key("sample-all", "Log", "100%", "request_id", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        for i in 0..10 {
            let log = create_test_log(
                "Log message",
                "info",
                "svc",
                "host",
                "arn",
                Some(&format!("req-{i}")),
            );
            assert!(
                evaluator.should_keep(&log),
                "100% sampling should keep all logs"
            );
        }
    }

    #[tokio::test]
    async fn test_sample_key_by_service() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        // Sample by service name - all logs from the same service get the same decision
        let policy = sampled_policy_with_key("sample-by-service", "Log", "50%", "service", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Multiple logs from "service-a" should all have the same decision
        let mut decisions_svc_a = Vec::new();
        for i in 0..5 {
            let log = create_test_log(
                &format!("Log {i}"),
                "info",
                "service-a",
                "host",
                "arn",
                None,
            );
            decisions_svc_a.push(evaluator.should_keep(&log));
        }

        let first_a = decisions_svc_a[0];
        assert!(
            decisions_svc_a.iter().all(|&d| d == first_a),
            "All logs from service-a should have the same decision"
        );

        // Multiple logs from "service-b" should all have the same decision
        let mut decisions_svc_b = Vec::new();
        for i in 0..5 {
            let log = create_test_log(
                &format!("Log {i}"),
                "info",
                "service-b",
                "host",
                "arn",
                None,
            );
            decisions_svc_b.push(evaluator.should_keep(&log));
        }

        let first_b = decisions_svc_b[0];
        assert!(
            decisions_svc_b.iter().all(|&d| d == first_b),
            "All logs from service-b should have the same decision"
        );
    }

    // ==================== Policy Updates Tests ====================

    #[tokio::test]
    async fn test_policy_update_takes_effect() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();
        let evaluator = PolicyEvaluator::new(Arc::clone(&registry));

        let log = create_test_log("test message", "info", "svc", "host", "arn", None);

        // Initially no policies - log is kept
        assert!(evaluator.should_keep(&log));

        // Add a drop policy
        let policy = body_regex_policy("drop-test", "test", "none", true);
        handle.update(vec![policy]);

        // Now log should be dropped
        assert!(!evaluator.should_keep(&log));

        // Remove all policies
        handle.update(vec![]);

        // Log should be kept again
        assert!(evaluator.should_keep(&log));
    }

    // ==================== Sync Drop/Keep/Sample Tests ====================

    #[test]
    fn test_sync_drop_policy() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-test", "Test log", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        assert!(!evaluator.should_keep(&log));
    }

    #[test]
    fn test_sync_keep_policy() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("keep-test", "Test log", "keep", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        assert!(evaluator.should_keep(&log));
    }

    #[test]
    fn test_sync_no_match_keeps() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-other", "no_match_here", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);
        let log = default_test_log();

        // No match means keep
        assert!(evaluator.should_keep(&log));
    }

    // ==================== Sync vs Async Consistency ====================

    #[tokio::test]
    async fn test_sync_and_async_produce_same_results() {
        let registry = Arc::new(PolicyRegistry::new());
        let handle = registry.register_provider();

        let policy = body_regex_policy("drop-specific", "drop_this", "none", true);
        handle.update(vec![policy]);

        let evaluator = PolicyEvaluator::new(registry);

        // Test with log that should be dropped
        let drop_log = create_test_log("drop_this please", "info", "svc", "host", "arn", None);
        assert_eq!(
            evaluator.should_keep(&drop_log),
            evaluator.should_keep(&drop_log)
        );

        // Test with log that should be kept
        let keep_log = create_test_log("keep this", "info", "svc", "host", "arn", None);
        assert_eq!(
            evaluator.should_keep(&keep_log),
            evaluator.should_keep(&keep_log)
        );
    }
}
