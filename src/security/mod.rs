// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Security subsystem for policy enforcement, sandboxing, and secret management.
//!
//! This module provides the security infrastructure for SenWeaverCoding. The core type
//! [`SecurityPolicy`] defines autonomy levels, workspace boundaries, and
//! access-control rules that are enforced across the tool and runtime subsystems.
//! [`PairingGuard`] implements device pairing for channel authentication, and
//! [`SecretStore`] handles encrypted credential storage.
//!
//! OS-level isolation is provided through the [`Sandbox`] trait defined in
//! [`traits`], with pluggable backends including Docker, Firejail, Bubblewrap,
//! and Landlock. The [`create_sandbox`] function selects the best available
//! backend at runtime. An [`AuditLogger`] records security-relevant events for
//! forensic review.
//!
//! # Extension
//!
//! To add a new sandbox backend, implement [`Sandbox`] in a new submodule and
//! register it in [`detect::create_sandbox`]. See `AGENTS.md` for security
//! change guidelines.

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
pub mod leak_detector;
pub mod leak_io;
pub mod nevis;
pub mod otp;
pub mod pairing;
pub mod permission_rules;
pub mod permissions;
pub mod playbook;
pub mod policy;
pub mod prompt_guard;
pub mod prompt_guard_yaml;
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

#[allow(unused_imports)]
pub use audit::{AuditEvent, AuditEventType, AuditLogger};
#[allow(unused_imports)]
pub use detect::create_sandbox;
pub use domain_matcher::DomainMatcher;
#[allow(unused_imports)]
pub use estop::{EstopLevel, EstopManager, EstopState, ResumeSelector};
#[allow(unused_imports)]
pub use otp::OtpValidator;
#[allow(unused_imports)]
pub use pairing::PairingGuard;
pub use policy::{AutonomyLevel, SecurityPolicy};
#[allow(unused_imports)]
pub use secrets::SecretStore;
#[allow(unused_imports)]
pub use traits::{NoopSandbox, Sandbox};

#[allow(unused_imports)]
pub use iam_policy::{IamPolicy, PolicyDecision};
#[allow(unused_imports)]
pub use nevis::{NevisAuthProvider, NevisIdentity};

#[allow(unused_imports)]
pub use capabilities::{Capability, CapabilityCheck, CapabilityError, CapabilityManager};
#[allow(unused_imports)]
pub use leak_detector::{LeakDetector, LeakResult};
#[allow(unused_imports)]
pub use manifest_signing::{ManifestSignError, ManifestSigner, SignedManifest};
#[allow(unused_imports)]
pub use prompt_guard::{GuardAction, GuardResult, PromptGuard};
#[allow(unused_imports)]
pub use rbac::{
    AccessContext, AuthSource, AuthorizationResult, CallerIdentity, RbacConfig, RbacEngine,
};
#[allow(unused_imports)]
pub use taint::{TaintLabel, TaintSink, TaintViolation, TaintedValue};
#[allow(unused_imports)]
pub use workspace_boundary::{BoundaryVerdict, WorkspaceBoundary};

pub use sandbox::{is_sandbox_active, sandbox_allows_path};

#[allow(unused_imports)]
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
