// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod audit;
#[cfg(feature = "sandbox-bubblewrap")]
pub mod bubblewrap;
pub mod detect;
pub mod docker;
pub mod secret_string;

pub mod domain_matcher;
pub mod estop;
#[cfg(target_os = "linux")]
pub mod firejail;
pub mod iam_policy;
#[cfg(feature = "sandbox-landlock")]
pub mod landlock;
pub mod leak;
pub mod nevis;
pub mod otp;
pub mod pairing;
pub mod permissions;
pub mod playbook;
pub mod policy;
pub mod prompt_guard;
pub mod safe_io;
#[cfg(target_os = "macos")]
pub mod seatbelt;
pub mod secrets;
pub mod traits;
pub mod vulnerability;
#[cfg(feature = "webauthn")]
pub mod webauthn;
pub mod workspace_boundary;

pub mod manifest_signing;

pub mod capabilities;
pub mod job_object;
pub mod rbac;
pub mod sandbox;
pub mod taint;

pub use audit::{
    AuditEvent, AuditEventType, AuditLogger, global_audit_logger, record_command_execution,
};
pub use detect::create_sandbox;
pub use domain_matcher::DomainMatcher;
pub use estop::{EstopLevel, EstopManager, EstopState, ResumeSelector};
pub use otp::OtpValidator;
pub use pairing::PairingGuard;
pub use policy::{is_system_path, AutonomyLevel, SecurityPolicy};
pub use secrets::SecretStore;
pub use traits::{NoopSandbox, Sandbox};

pub use iam_policy::{IamPolicy, PolicyDecision};
pub use nevis::{NevisAuthProvider, NevisIdentity};

pub use capabilities::{Capability, CapabilityCheck, CapabilityError, CapabilityManager};
pub use leak::detector::{LeakDetector, LeakResult};
pub use manifest_signing::{ManifestSignError, ManifestSigner, SignedManifest};
pub use prompt_guard::{GuardAction, GuardResult, PromptGuard};
pub use rbac::{
    AccessContext, AuthSource, AuthorizationResult, CallerIdentity, RbacConfig, RbacEngine,
};
pub use taint::{TaintLabel, TaintSink, TaintViolation, TaintedValue};
pub use workspace_boundary::{BoundaryVerdict, WorkspaceBoundary};

pub use sandbox::{
    configure_fs_confinement, is_sandbox_active, register_workspace_root, sandbox_allows_path,
};

pub use job_object::{spawn_in_job, JobLimits, JobObjectGuard, JobObjectSandbox};

pub fn redact(value: &str) -> String {
    let char_count = value.chars().count();
    if char_count <= 4 {
        "***".to_string()
    } else {
        let prefix: String = value.chars().take(4).collect();
        format!("{prefix}***")
    }
}
