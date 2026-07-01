//! Matchable trait implementations for bottlecap telemetry types.
//!
//! This module provides implementations of the `policy_rs::Matchable` trait
//! for `IntakeLog` (logs) and `SpanWrapper` (traces), enabling policy evaluation
//! against these telemetry types.

use std::borrow::Cow;

use policy_rs::proto::tero::policy::v1::LogField;
use policy_rs::{LogFieldSelector, Matchable};

use crate::logs::lambda::IntakeLog;
use libdd_trace_protobuf::pb;

impl Matchable for IntakeLog {
    fn get_field(&self, field: &LogFieldSelector) -> Option<Cow<'_, str>> {
        match field {
            LogFieldSelector::Simple(simple) => match simple {
                LogField::Body => Some(Cow::Borrowed(&self.message.message)),
                LogField::SeverityText => Some(Cow::Borrowed(&self.message.status)),
                _ => None,
            },
            LogFieldSelector::ResourceAttribute(key) => {
                // key is a Vec<String> representing a path; we match on the first element
                let first_key = key.first().map(String::as_str)?;
                match first_key {
                    "service" => Some(Cow::Borrowed(&self.service)),
                    "host" | "hostname" => Some(Cow::Borrowed(&self.hostname)),
                    "source" => Some(Cow::Borrowed(&self.source)),
                    "arn" => Some(Cow::Borrowed(&self.message.lambda.arn)),
                    "request_id" => self.message.lambda.request_id.as_deref().map(Cow::Borrowed),
                    _ => None,
                }
            }
            // Parse tags on-the-fly: tags are stored as "key1:value1,key2:value2"
            LogFieldSelector::LogAttribute(key) => {
                let first_key = key.first().map(String::as_str)?;
                self.tags.split(',').find_map(|tag| {
                    let (k, v) = tag.split_once(':')?;
                    if k == first_key {
                        Some(Cow::Owned(v.to_string()))
                    } else {
                        None
                    }
                })
            }
            LogFieldSelector::ScopeAttribute(_) => None,
        }
    }
}

/// Wrapper around `pb::Span` to implement `Matchable` trait.
///
/// Due to Rust's orphan rules, we cannot implement a foreign trait (`Matchable`)
/// for a foreign type (`pb::Span`). This newtype wrapper allows us to implement
/// the trait while maintaining zero-cost abstraction.
pub struct SpanWrapper<'a>(pub &'a pb::Span);

impl Matchable for SpanWrapper<'_> {
    fn get_field(&self, field: &LogFieldSelector) -> Option<Cow<'_, str>> {
        match field {
            LogFieldSelector::Simple(simple) => match simple {
                LogField::Body => Some(Cow::Borrowed(&self.0.resource)),
                _ => None,
            },
            LogFieldSelector::ResourceAttribute(key) => {
                // key is a Vec<String> representing a path; we match on the first element
                let first_key = key.first().map(String::as_str)?;
                match first_key {
                    "service" => Some(Cow::Borrowed(&self.0.service)),
                    "name" => Some(Cow::Borrowed(&self.0.name)),
                    "resource" => Some(Cow::Borrowed(&self.0.resource)),
                    "type" => Some(Cow::Borrowed(&self.0.r#type)),
                    _ => self
                        .0
                        .meta
                        .get(first_key)
                        .map(|s| Cow::Borrowed(s.as_str())),
                }
            }
            LogFieldSelector::LogAttribute(key) => {
                let first_key = key.first()?;
                self.0
                    .meta
                    .get(first_key)
                    .map(|s| Cow::Borrowed(s.as_str()))
            }
            LogFieldSelector::ScopeAttribute(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::lambda::{Lambda, Message};
    use std::collections::HashMap;

    fn create_test_intake_log() -> IntakeLog {
        IntakeLog {
            message: Message {
                message: "Test log message".to_string(),
                lambda: Lambda {
                    arn: "arn:aws:lambda:us-east-1:123456789:function:test".to_string(),
                    request_id: Some("req-123".to_string()),
                    durable_execution_id: None,
                    durable_execution_name: None,
                    first_invocation: None,
                    durable_execution_status: None,
                },
                timestamp: 1_234_567_890,
                status: "info".to_string(),
            },
            hostname: "test-host".to_string(),
            service: "test-service".to_string(),
            tags: "env:test,region:us-east-1".to_string(),
            source: "lambda".to_string(),
        }
    }

    fn create_test_span() -> pb::Span {
        let mut meta = HashMap::new();
        meta.insert("env".to_string(), "production".to_string());
        meta.insert("custom_tag".to_string(), "custom_value".to_string());

        pb::Span {
            name: "test.span".to_string(),
            service: "test-service".to_string(),
            resource: "/api/users".to_string(),
            r#type: "web".to_string(),
            meta,
            ..Default::default()
        }
    }

    #[test]
    fn test_intake_log_body() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::Simple(LogField::Body))
                .as_deref(),
            Some("Test log message")
        );
    }

    #[test]
    fn test_intake_log_severity() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::Simple(LogField::SeverityText))
                .as_deref(),
            Some("info")
        );
    }

    #[test]
    fn test_intake_log_resource_attributes() {
        let log = create_test_intake_log();

        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "service".to_string()
            ]))
            .as_deref(),
            Some("test-service")
        );
        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "hostname".to_string()
            ]))
            .as_deref(),
            Some("test-host")
        );
        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "arn".to_string()
            ]))
            .as_deref(),
            Some("arn:aws:lambda:us-east-1:123456789:function:test")
        );
        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "request_id".to_string()
            ]))
            .as_deref(),
            Some("req-123")
        );
    }

    #[test]
    fn test_intake_log_missing_request_id() {
        let mut log = create_test_intake_log();
        log.message.lambda.request_id = None;

        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "request_id".to_string()
            ]))
            .as_deref(),
            None
        );
    }

    #[test]
    fn test_intake_log_tag_attribute() {
        let log = create_test_intake_log();

        // Should find "env" tag with value "test"
        assert_eq!(
            log.get_field(&LogFieldSelector::LogAttribute(vec!["env".to_string()]))
                .as_deref(),
            Some("test")
        );

        // Should find "region" tag with value "us-east-1"
        assert_eq!(
            log.get_field(&LogFieldSelector::LogAttribute(vec!["region".to_string()]))
                .as_deref(),
            Some("us-east-1")
        );

        // Should return None for non-existent tag
        assert_eq!(
            log.get_field(&LogFieldSelector::LogAttribute(vec![
                "nonexistent".to_string()
            ]))
            .as_deref(),
            None
        );
    }

    #[test]
    fn test_span_body() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::Simple(LogField::Body))
                .as_deref(),
            Some("/api/users")
        );
    }

    #[test]
    fn test_span_resource_attributes() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);

        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ResourceAttribute(vec![
                    "service".to_string()
                ]))
                .as_deref(),
            Some("test-service")
        );
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ResourceAttribute(vec![
                    "name".to_string()
                ]))
                .as_deref(),
            Some("test.span")
        );
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ResourceAttribute(vec![
                    "type".to_string()
                ]))
                .as_deref(),
            Some("web")
        );
    }

    #[test]
    fn test_span_meta_via_resource_attribute() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);

        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ResourceAttribute(vec![
                    "env".to_string()
                ]))
                .as_deref(),
            Some("production")
        );
    }

    #[test]
    fn test_span_log_attribute() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);

        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::LogAttribute(vec![
                    "custom_tag".to_string()
                ]))
                .as_deref(),
            Some("custom_value")
        );
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::LogAttribute(vec![
                    "nonexistent".to_string()
                ]))
                .as_deref(),
            None
        );
    }

    // IntakeLog coverage gaps

    #[test]
    fn test_intake_log_source_resource_attribute() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "source".to_string()
            ]))
            .as_deref(),
            Some("lambda")
        );
    }

    #[test]
    fn test_intake_log_host_alias() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "host".to_string()
            ]))
            .as_deref(),
            Some("test-host")
        );
    }

    #[test]
    fn test_intake_log_unknown_resource_attribute() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::ResourceAttribute(vec![
                "unknown_field".to_string()
            ]))
            .as_deref(),
            None
        );
    }

    #[test]
    fn test_intake_log_scope_attribute_returns_none() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::ScopeAttribute(vec![
                "anything".to_string()
            ]))
            .as_deref(),
            None
        );
    }

    #[test]
    fn test_intake_log_unsupported_simple_field_returns_none() {
        let log = create_test_intake_log();
        assert_eq!(
            log.get_field(&LogFieldSelector::Simple(LogField::Unspecified))
                .as_deref(),
            None
        );
    }

    // SpanWrapper coverage gaps

    #[test]
    fn test_span_resource_field() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ResourceAttribute(vec![
                    "resource".to_string()
                ]))
                .as_deref(),
            Some("/api/users")
        );
    }

    #[test]
    fn test_span_unknown_resource_attribute() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ResourceAttribute(vec![
                    "unknown_field".to_string()
                ]))
                .as_deref(),
            None
        );
    }

    #[test]
    fn test_span_scope_attribute_returns_none() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::ScopeAttribute(vec![
                    "anything".to_string()
                ]))
                .as_deref(),
            None
        );
    }

    #[test]
    fn test_span_unsupported_simple_field_returns_none() {
        let span = create_test_span();
        let wrapper = SpanWrapper(&span);
        assert_eq!(
            wrapper
                .get_field(&LogFieldSelector::Simple(LogField::SeverityText))
                .as_deref(),
            None
        );
    }
}
