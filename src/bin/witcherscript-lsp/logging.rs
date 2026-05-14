use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tower_lsp::lsp_types::MessageType;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;

pub(crate) const LEVEL_ERROR: u8 = 1;
pub(crate) const LEVEL_WARN: u8 = 2;
pub(crate) const LEVEL_INFO: u8 = 3;
pub(crate) const LEVEL_DEBUG: u8 = 4;
pub(crate) const LEVEL_TRACE: u8 = 5;

pub(crate) const DEFAULT_LOG_LEVEL: tracing::Level = tracing::Level::WARN;

pub(crate) fn level_to_u8(level: tracing::Level) -> u8 {
    match level {
        tracing::Level::ERROR => LEVEL_ERROR,
        tracing::Level::WARN => LEVEL_WARN,
        tracing::Level::INFO => LEVEL_INFO,
        tracing::Level::DEBUG => LEVEL_DEBUG,
        tracing::Level::TRACE => LEVEL_TRACE,
    }
}

pub(crate) fn level_from_u8(n: u8) -> tracing::Level {
    match n {
        LEVEL_ERROR => tracing::Level::ERROR,
        LEVEL_WARN => tracing::Level::WARN,
        LEVEL_INFO => tracing::Level::INFO,
        LEVEL_DEBUG => tracing::Level::DEBUG,
        LEVEL_TRACE => tracing::Level::TRACE,
        _ => DEFAULT_LOG_LEVEL,
    }
}

pub(crate) fn level_from_str(s: &str) -> tracing::Level {
    match s.to_ascii_lowercase().as_str() {
        "error" => tracing::Level::ERROR,
        "warn" | "warning" => tracing::Level::WARN,
        "debug" => tracing::Level::DEBUG,
        "trace" => tracing::Level::TRACE,
        _ => DEFAULT_LOG_LEVEL,
    }
}

/// Forwards tracing events to the LSP client's log_message via an async channel.
/// The `min_level` atomic is updated at runtime when the user changes `witcherscript.logLevel`.
pub(crate) struct LspLogSender {
    pub(crate) sender: mpsc::UnboundedSender<(MessageType, String)>,
    pub(crate) min_level: Arc<AtomicU8>,
}

const OWN_TARGET_PREFIXES: [&str; 2] = ["witcherscript_lsp", "witcherscript_parser"];

fn is_own_target(target: &str) -> bool {
    OWN_TARGET_PREFIXES.iter().any(|prefix| {
        target == *prefix
            || target
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with("::"))
    })
}

impl<S: tracing::Subscriber> Layer<S> for LspLogSender {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !is_own_target(event.metadata().target()) {
            return;
        }

        let level = *event.metadata().level();
        if level > level_from_u8(self.min_level.load(Ordering::Relaxed)) {
            return;
        }

        let msg_type = match level {
            tracing::Level::ERROR => MessageType::ERROR,
            tracing::Level::WARN => MessageType::WARNING,
            tracing::Level::INFO => MessageType::INFO,
            _ => MessageType::LOG,
        };

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let message = if msg_type == MessageType::LOG {
            format!("[{}] {}", utc_timestamp(), visitor.finish())
        } else {
            visitor.finish()
        };
        let _ = self.sender.send((msg_type, message));
    }
}

fn utc_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}.{:03}", now.subsec_millis())
}

#[derive(Default)]
struct EventVisitor {
    message: String,
    fields: String,
}

impl EventVisitor {
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else {
            format!("{} {}", self.message, self.fields)
        }
    }

    fn push_field(&mut self, name: &str, value: &dyn std::fmt::Display) {
        if !self.fields.is_empty() {
            self.fields.push(' ');
        }
        self.fields.push_str(&format!("{name}={value}"));
    }
}

impl Visit for EventVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_owned();
        } else {
            self.push_field(field.name(), &value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.push_field(field.name(), &format_args!("{value:?}"));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push_field(field.name(), &value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push_field(field.name(), &value);
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.push_field(field.name(), &value);
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.push_field(field.name(), &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push_field(field.name(), &value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_targets_pass_dependency_targets_are_rejected() {
        assert!(is_own_target("witcherscript_lsp"));
        assert!(is_own_target("witcherscript_lsp::indexing"));
        assert!(is_own_target("witcherscript_parser::resolve"));
        assert!(!is_own_target("tower_lsp::jsonrpc"));
        assert!(!is_own_target("hyper::proto"));
        assert!(!is_own_target("witcherscript_lsp_extra"));
    }

    #[test]
    fn utc_timestamp_has_millisecond_granularity() {
        let ts = utc_timestamp();
        assert_eq!(ts.len(), 12, "expected HH:MM:SS.mmm, got {ts}");
        let (time, millis) = ts.split_once('.').expect("missing millisecond component");
        assert_eq!(millis.len(), 3);
        assert!(millis.chars().all(|c| c.is_ascii_digit()));
        let parts: Vec<&str> = time.split(':').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit())));
    }

    #[test]
    fn level_u8_round_trip_covers_every_level() {
        for level in [
            tracing::Level::ERROR,
            tracing::Level::WARN,
            tracing::Level::INFO,
            tracing::Level::DEBUG,
            tracing::Level::TRACE,
        ] {
            assert_eq!(
                level_from_u8(level_to_u8(level)),
                level,
                "round-trip lost level {level}"
            );
        }
    }
}
