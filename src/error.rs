// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    Network,
    Timeout,
    RateLimit,
    Permission,
    Validation,
    NotFound,
    Provider,
    Storage,
    Internal,
    Cancelled,
}

impl ErrorCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCategory::Network => "network",
            ErrorCategory::Timeout => "timeout",
            ErrorCategory::RateLimit => "rate_limit",
            ErrorCategory::Permission => "permission",
            ErrorCategory::Validation => "validation",
            ErrorCategory::NotFound => "not_found",
            ErrorCategory::Provider => "provider",
            ErrorCategory::Storage => "storage",
            ErrorCategory::Internal => "internal",
            ErrorCategory::Cancelled => "cancelled",
        }
    }
}

pub trait ErrorClassification {
    fn category(&self) -> ErrorCategory;

    fn is_retryable(&self) -> bool {
        matches!(
            self.category(),
            ErrorCategory::Network
                | ErrorCategory::Timeout
                | ErrorCategory::RateLimit
                | ErrorCategory::Storage
        )
    }

    fn is_fatal(&self) -> bool {
        matches!(
            self.category(),
            ErrorCategory::Permission | ErrorCategory::Validation | ErrorCategory::Cancelled
        )
    }

    fn retry_after_hint(&self) -> Option<std::time::Duration> {
        None
    }
}

pub fn extract_http_status_code(msg: &str) -> Option<u16> {
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - start == 3 {
                if let Ok(code) = msg[start..i].parse::<u16>() {
                    if (100..=599).contains(&code)
                        && (has_status_context_before(msg, start)
                            || has_reason_phrase_after(msg, i))
                    {
                        return Some(code);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn preceding_word(msg: &str, mut end: usize) -> (Option<String>, usize) {
    let bytes = msg.as_bytes();
    while end > 0 && !bytes[end - 1].is_ascii_alphanumeric() {
        end -= 1;
    }
    let word_end = end;
    while end > 0 && bytes[end - 1].is_ascii_alphabetic() {
        end -= 1;
    }
    if end == word_end {
        return (None, end);
    }
    (Some(msg[end..word_end].to_ascii_lowercase()), end)
}

fn has_status_context_before(msg: &str, digit_start: usize) -> bool {
    let (word, word_start) = preceding_word(msg, digit_start);
    let Some(word) = word else { return false };
    match word.as_str() {
        "http" | "status" | "code" | "statuscode" => true,
        "error" => {
            let (prev, _) = preceding_word(msg, word_start);
            prev.as_deref() != Some("os")
        }
        _ => false,
    }
}

fn has_reason_phrase_after(msg: &str, digit_end: usize) -> bool {
    let rest = msg[digit_end..]
        .trim_start_matches([' ', '-', ':', '(', ')', '.', ','])
        .to_ascii_lowercase();
    const REASON_PHRASES: &[&str] = &[
        "bad request",
        "unauthorized",
        "payment required",
        "forbidden",
        "not found",
        "method not allowed",
        "not acceptable",
        "proxy authentication required",
        "request timeout",
        "conflict",
        "gone",
        "length required",
        "precondition failed",
        "payload too large",
        "request entity too large",
        "uri too long",
        "unsupported media type",
        "unprocessable entity",
        "too many requests",
        "unavailable for legal reasons",
        "internal server error",
        "bad gateway",
        "service unavailable",
        "gateway timeout",
    ];
    REASON_PHRASES.iter().any(|phrase| rest.starts_with(phrase))
}

pub fn classify_anyhow(err: &anyhow::Error) -> ErrorCategory {
    for cause in err.chain() {
        if cause.downcast_ref::<tokio::time::error::Elapsed>().is_some() {
            return ErrorCategory::Timeout;
        }
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            return io_error_category(io_err);
        }
        if let Some(provider_err) = cause.downcast_ref::<crate::providers::ProviderError>() {
            return provider_error_category(provider_err);
        }
        if let Some(stream_err) =
            cause.downcast_ref::<crate::providers::traits::StreamError>()
        {
            return stream_err.category();
        }
        if let Some(reqwest_err) = cause.downcast_ref::<reqwest::Error>() {
            return reqwest_error_category(reqwest_err);
        }
        if let Some(agent_err) = cause.downcast_ref::<AgentError>() {
            return agent_err.category();
        }
        if let Some(sen_err) = cause.downcast_ref::<SenError>() {
            return sen_err.category();
        }
    }
    let s = err.to_string().to_lowercase();
    let status = extract_http_status_code(&s);
    if s.contains("timed out") || s.contains("timeout") || s.contains("deadline") {
        return ErrorCategory::Timeout;
    }
    if status == Some(429) || s.contains("too_many_requests") {
        return ErrorCategory::RateLimit;
    }
    if s.contains("unauthorized") || s.contains("forbidden") || status == Some(401) || status == Some(403) || s.contains("permission") {
        return ErrorCategory::Permission;
    }
    if s.contains("not found") || status == Some(404) {
        return ErrorCategory::NotFound;
    }
    if s.contains("connection") || s.contains("dns") || s.contains("network") || s.contains("reset by peer") || s.contains("broken pipe") {
        return ErrorCategory::Network;
    }
    if s.contains("database is locked") || s.contains("database is busy") || s.contains("sqlite_busy") {
        return ErrorCategory::Storage;
    }
    if s.contains("cancel") {
        return ErrorCategory::Cancelled;
    }
    if s.contains("validation") || s.contains("invalid") {
        return ErrorCategory::Validation;
    }
    if s.contains("provider") || s.contains("model") {
        return ErrorCategory::Provider;
    }
    ErrorCategory::Internal
}

fn io_error_category(err: &std::io::Error) -> ErrorCategory {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::TimedOut => ErrorCategory::Timeout,
        ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::BrokenPipe
        | ErrorKind::AddrInUse
        | ErrorKind::AddrNotAvailable
        | ErrorKind::HostUnreachable
        | ErrorKind::NetworkUnreachable
        | ErrorKind::NetworkDown => ErrorCategory::Network,
        ErrorKind::PermissionDenied => ErrorCategory::Permission,
        ErrorKind::NotFound => ErrorCategory::NotFound,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => ErrorCategory::Validation,
        ErrorKind::Interrupted => ErrorCategory::Cancelled,
        _ => ErrorCategory::Storage,
    }
}

fn reqwest_error_category(err: &reqwest::Error) -> ErrorCategory {
    if err.is_timeout() {
        return ErrorCategory::Timeout;
    }
    if err.is_connect() {
        return ErrorCategory::Network;
    }
    if let Some(status) = err.status() {
        let code = status.as_u16();
        if code == 401 || code == 403 {
            return ErrorCategory::Permission;
        }
        if code == 404 {
            return ErrorCategory::NotFound;
        }
        if code == 429 {
            return ErrorCategory::RateLimit;
        }
        if (500..=599).contains(&code) {
            return ErrorCategory::Provider;
        }
        if (400..500).contains(&code) {
            return ErrorCategory::Validation;
        }
    }
    if err.is_request() || err.is_body() || err.is_decode() {
        return ErrorCategory::Provider;
    }
    ErrorCategory::Network
}

pub fn provider_error_category(err: &crate::providers::ProviderError) -> ErrorCategory {
    use crate::services::api::ApiErrorCategory;
    match err.category() {
        ApiErrorCategory::ServerError => ErrorCategory::Provider,
        ApiErrorCategory::RateLimited | ApiErrorCategory::Overloaded => ErrorCategory::RateLimit,
        ApiErrorCategory::AuthError => ErrorCategory::Permission,
        ApiErrorCategory::InvalidRequest | ApiErrorCategory::ContextLengthExceeded => {
            ErrorCategory::Validation
        }
        ApiErrorCategory::NetworkError => ErrorCategory::Network,
        ApiErrorCategory::Timeout => ErrorCategory::Timeout,
        ApiErrorCategory::Unknown => ErrorCategory::Provider,
    }
}

impl ErrorClassification for crate::providers::ProviderError {
    fn category(&self) -> ErrorCategory {
        provider_error_category(self)
    }
}

impl ErrorClassification for std::io::Error {
    fn category(&self) -> ErrorCategory {
        io_error_category(self)
    }
}

impl ErrorClassification for anyhow::Error {
    fn category(&self) -> ErrorCategory {
        classify_anyhow(self)
    }
}

#[derive(Debug, Error)]
pub enum SenError {
    #[error("agent error: {0}")]
    Agent(#[from] AgentError),

    #[error("scheduler error: {0}")]
    Scheduler(#[from] SchedulerError),

    #[error("coordinator error: {0}")]
    Coordinator(#[from] CoordinatorError),

    #[error("blackboard error: {0}")]
    Blackboard(#[from] BlackboardError),

    #[error("event bus error: {0}")]
    EventBus(#[from] EventBusError),

    #[error("supervisor error: {0}")]
    Supervisor(#[from] SupervisorError),

    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("task queue error: {0}")]
    TaskQueue(#[from] TaskQueueError),

    #[error("config error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum AgentError {

    #[error("agent exceeded maximum tool iterations ({0})")]
    LoopOverflow(usize),

    #[error("model switch failed: {0}")]
    ModelSwitchFailed(String),

    #[error("turn was cancelled")]
    TurnCancelled,

    #[error("tool dispatch failed: {0}")]
    ToolDispatchFailed(String),

    #[error("stream interrupted: {0}")]
    StreamInterrupted(String),

    #[error("context budget exceeded: {0}")]
    ContextBudgetExceeded(String),

    #[error("cost budget exceeded: {0}")]
    CostBudgetExceeded(String),

    #[error("loop aborted by detector: {0}")]
    LoopAborted(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("tool `{tool_name}` failed: {cause}")]
    Tool {
        tool_name: String,
        #[source]
        cause: crate::tools::ToolErrorCause,
    },
}

impl AgentError {

    pub fn tool_failed(tool_name: impl Into<String>, cause: crate::tools::ToolErrorCause) -> Self {
        AgentError::Tool {
            tool_name: tool_name.into(),
            cause,
        }
    }

    pub fn tool_name(&self) -> Option<&str> {
        match self {
            AgentError::Tool { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        }
    }

    pub fn cause(&self) -> Option<&crate::tools::ToolErrorCause> {
        match self {
            AgentError::Tool { cause, .. } => Some(cause),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for AgentError {

    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<std::io::Error>() {
            Ok(io_err) => {
                return AgentError::Tool {
                    tool_name: "<unknown>".into(),
                    cause: crate::tools::ToolErrorCause::Io(io_err),
                };
            }
            Err(e) => {
                if e.downcast_ref::<tokio::time::error::Elapsed>().is_some() {
                    return AgentError::Tool {
                        tool_name: "<unknown>".into(),
                        cause: crate::tools::ToolErrorCause::Timeout(
                            std::time::Duration::from_secs(0),
                        ),
                    };
                }
                AgentError::Tool {
                    tool_name: "<unknown>".into(),
                    cause: crate::tools::ToolErrorCause::Unknown(e),
                }
            }
        }
    }
}

impl From<String> for AgentError {
    fn from(s: String) -> Self {
        AgentError::ToolDispatchFailed(s)
    }
}

impl From<&str> for AgentError {
    fn from(s: &str) -> Self {
        AgentError::ToolDispatchFailed(s.to_string())
    }
}

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("dependency cycle detected in task graph")]
    CycleDetected,

    #[error("unknown dependency: task '{task}' depends on '{dependency}'")]
    UnknownDependency { task: String, dependency: String },

    #[error("task '{0}' not found")]
    TaskNotFound(String),

    #[error("scheduler cancelled")]
    Cancelled,
}

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("lock contention on resource '{resource}' by agent '{agent}'")]
    LockContention { resource: String, agent: String },

    #[error("barrier '{0}' timed out")]
    BarrierTimeout(String),

    #[error("voting session '{0}' expired")]
    VotingExpired(String),

    #[error("agent '{0}' not registered")]
    AgentNotFound(String),
}

#[derive(Debug, Error)]
pub enum BlackboardError {
    #[error("key '{0}' not found")]
    KeyNotFound(String),

    #[error("write conflict on key '{0}' (version mismatch)")]
    VersionConflict(String),

    #[error("entry expired")]
    Expired,
}

#[derive(Debug, Error)]
pub enum EventBusError {
    #[error("channel closed")]
    ChannelClosed,

    #[error("subscriber lagged behind by {0} events")]
    Lagged(u64),
}

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("maximum agent limit ({0}) reached")]
    MaxAgentsLimit(usize),

    #[error("capability '{0}' agent limit ({1}) reached")]
    CapabilityLimit(String, usize),

    #[error("agent '{0}' already registered")]
    AlreadyRegistered(String),

    #[error("agent '{0}' not found")]
    AgentNotFound(String),
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("agent '{0}' already registered")]
    AlreadyRegistered(String),

    #[error("agent '{0}' not found")]
    AgentNotFound(String),

    #[error("agent '{0}' is not available for task assignment (state: {1})")]
    AgentNotAvailable(String, String),

    #[error("registry limit exceeded: max_agents={0}")]
    MaxAgentsLimit(usize),

    #[error("registry capability '{0}' limit {1} exceeded")]
    CapabilityLimit(String, usize),

    #[error("agent '{agent_id}' not in expected state: expected {expected}, found {found}")]
    StateMismatch {
        agent_id: String,
        expected: String,
        found: String,
    },
}

impl From<String> for RegistryError {
    fn from(s: String) -> Self {

        if s.contains("not found") || s.contains("not_registered") {
            RegistryError::AgentNotFound(s)
        } else if s.contains("already registered") {
            RegistryError::AlreadyRegistered(s)
        } else {
            RegistryError::AgentNotFound(s)
        }
    }
}

#[derive(Debug, Error)]
pub enum TaskQueueError {
    #[error("task '{0}' not found")]
    TaskNotFound(String),

    #[error("task '{task_id}' not in expected status: expected {expected}, found {found}")]
    StatusMismatch {
        task_id: String,
        expected: String,
        found: String,
    },

    #[error("task '{0}' not in running state")]
    NotRunning(String),

    #[error("queue capacity exceeded")]
    CapacityExceeded,

    #[error(
        "task '{task_id}' claim is stale: reported by '{reporter}' (attempt {reported_attempt}) \
         but currently held by '{holder}' (attempt {current_attempt})"
    )]
    StaleClaim {
        task_id: String,
        reporter: String,
        reported_attempt: u32,
        holder: String,
        current_attempt: u32,
    },
}

impl ErrorClassification for SenError {
    fn category(&self) -> ErrorCategory {
        match self {
            SenError::Agent(e) => e.category(),
            SenError::Scheduler(e) => e.category(),
            SenError::Coordinator(e) => e.category(),
            SenError::Blackboard(e) => e.category(),
            SenError::EventBus(e) => e.category(),
            SenError::Supervisor(e) => e.category(),
            SenError::Registry(e) => e.category(),
            SenError::TaskQueue(e) => e.category(),
            SenError::Config(_) => ErrorCategory::Validation,
            SenError::Provider(_) => ErrorCategory::Provider,
            SenError::Other(e) => classify_anyhow(e),
        }
    }
}

impl ErrorClassification for AgentError {
    fn category(&self) -> ErrorCategory {
        match self {
            AgentError::LoopOverflow(_) | AgentError::LoopAborted(_) => ErrorCategory::Internal,
            AgentError::ModelSwitchFailed(_) | AgentError::Provider(_) => ErrorCategory::Provider,
            AgentError::TurnCancelled => ErrorCategory::Cancelled,
            AgentError::ToolDispatchFailed(s) => {
                let lower = s.to_lowercase();
                if lower.contains("timeout") || lower.contains("timed out") {
                    ErrorCategory::Timeout
                } else if lower.contains("cancel") {
                    ErrorCategory::Cancelled
                } else {
                    ErrorCategory::Internal
                }
            }
            AgentError::StreamInterrupted(_) => ErrorCategory::Network,
            AgentError::ContextBudgetExceeded(_) | AgentError::CostBudgetExceeded(_) => {
                ErrorCategory::Validation
            }
            AgentError::Tool { cause, .. } => cause.category(),
        }
    }
}

impl ErrorClassification for crate::tools::ToolErrorCause {
    fn category(&self) -> ErrorCategory {
        use crate::tools::ToolErrorCause as C;
        match self {
            C::Validation(_) | C::PreconditionFailed(_) => ErrorCategory::Validation,
            C::Execution(_) => ErrorCategory::Internal,
            C::Timeout(_) => ErrorCategory::Timeout,
            C::Cancelled => ErrorCategory::Cancelled,
            C::RbacDenied(_) => ErrorCategory::Permission,
            C::Io(e) => io_error_category(e),
            C::Provider(_) => ErrorCategory::Provider,
            C::LockContention(_) => ErrorCategory::Storage,
            C::NoMatchingAgent(_) => ErrorCategory::NotFound,
            C::Unknown(e) => classify_anyhow(e),
        }
    }
}

impl ErrorClassification for SchedulerError {
    fn category(&self) -> ErrorCategory {
        match self {
            SchedulerError::CycleDetected | SchedulerError::UnknownDependency { .. } => {
                ErrorCategory::Validation
            }
            SchedulerError::TaskNotFound(_) => ErrorCategory::NotFound,
            SchedulerError::Cancelled => ErrorCategory::Cancelled,
        }
    }
}

impl ErrorClassification for CoordinatorError {
    fn category(&self) -> ErrorCategory {
        match self {
            CoordinatorError::LockContention { .. } => ErrorCategory::Storage,
            CoordinatorError::BarrierTimeout(_) | CoordinatorError::VotingExpired(_) => {
                ErrorCategory::Timeout
            }
            CoordinatorError::AgentNotFound(_) => ErrorCategory::NotFound,
        }
    }
}

impl ErrorClassification for BlackboardError {
    fn category(&self) -> ErrorCategory {
        match self {
            BlackboardError::KeyNotFound(_) => ErrorCategory::NotFound,
            BlackboardError::VersionConflict(_) => ErrorCategory::Storage,
            BlackboardError::Expired => ErrorCategory::Timeout,
        }
    }
}

impl ErrorClassification for EventBusError {
    fn category(&self) -> ErrorCategory {
        match self {
            EventBusError::ChannelClosed => ErrorCategory::Cancelled,
            EventBusError::Lagged(_) => ErrorCategory::Internal,
        }
    }
}

impl ErrorClassification for SupervisorError {
    fn category(&self) -> ErrorCategory {
        match self {
            SupervisorError::MaxAgentsLimit(_) | SupervisorError::CapabilityLimit(_, _) => {
                ErrorCategory::Validation
            }
            SupervisorError::AlreadyRegistered(_) => ErrorCategory::Storage,
            SupervisorError::AgentNotFound(_) => ErrorCategory::NotFound,
        }
    }
}

impl ErrorClassification for RegistryError {
    fn category(&self) -> ErrorCategory {
        match self {
            RegistryError::AlreadyRegistered(_) => ErrorCategory::Storage,
            RegistryError::AgentNotFound(_) => ErrorCategory::NotFound,
            RegistryError::AgentNotAvailable(_, _)
            | RegistryError::StateMismatch { .. }
            | RegistryError::MaxAgentsLimit(_)
            | RegistryError::CapabilityLimit(_, _) => ErrorCategory::Validation,
        }
    }
}

impl ErrorClassification for TaskQueueError {
    fn category(&self) -> ErrorCategory {
        match self {
            TaskQueueError::TaskNotFound(_) => ErrorCategory::NotFound,
            TaskQueueError::StatusMismatch { .. }
            | TaskQueueError::NotRunning(_)
            | TaskQueueError::StaleClaim { .. } => ErrorCategory::Validation,
            TaskQueueError::CapacityExceeded => ErrorCategory::Internal,
        }
    }
}
