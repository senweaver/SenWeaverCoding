// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::time::timeout;

use super::traits::{
    Artifact, IssueSeverity, Language, VerificationIssue, VerificationReport, Verifier,
};

#[derive(Debug, Clone)]
pub struct TestRunnerConfig {

    pub timeout: Duration,

    pub stderr_tail_chars: usize,

    pub cwd_override: Option<PathBuf>,

    pub dry_run: bool,

    pub heavy: bool,

    pub max_diagnostics: usize,
}

impl Default for TestRunnerConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(90),
            stderr_tail_chars: 4_096,
            cwd_override: None,
            dry_run: false,
            heavy: false,
            max_diagnostics: 50,
        }
    }
}

#[derive(Debug, Clone)]
struct CommandSpec {
    program: &'static str,
    args: Vec<String>,

    parser: ParserKind,

    cwd: PathBuf,

    label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserKind {

    CargoJson,

    StderrTail,
}

#[derive(Debug, Clone)]
pub struct TestRunnerBuilder {
    workspace_root: PathBuf,
    config: TestRunnerConfig,
}

impl TestRunnerBuilder {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            config: TestRunnerConfig::default(),
        }
    }

    pub fn heavy(mut self, on: bool) -> Self {
        self.config.heavy = on;
        self
    }

    pub fn with_config(mut self, cfg: TestRunnerConfig) -> Self {
        self.config = cfg;
        self
    }

    pub fn build(self) -> Vec<Box<dyn Verifier>> {
        let mut out: Vec<Box<dyn Verifier>> = Vec::new();
        let root = self.workspace_root.clone();
        let cfg = self.config.clone();
        let heavy = cfg.heavy;

        if root.join("Cargo.toml").is_file() {
            let mut args: Vec<String> = if heavy {
                vec![
                    "test".into(),
                    "--no-run".into(),
                    "--message-format=json".into(),
                    "--quiet".into(),
                ]
            } else {
                vec![
                    "check".into(),
                    "--message-format=json".into(),
                    "--quiet".into(),
                ]
            };

            args.push("--color".into());
            args.push("never".into());
            out.push(Box::new(TestRunnerVerifier {
                config: cfg.clone(),
                command: Some(CommandSpec {
                    program: "cargo",
                    args,
                    parser: ParserKind::CargoJson,
                    cwd: root.clone(),
                    label: if heavy { "cargo-test-no-run" } else { "cargo-check" },
                }),
            }));
        }

        if root.join("package.json").is_file() {
            let (program, args, label): (&'static str, Vec<String>, &'static str) = if heavy {
                ("npm", vec!["test".into(), "--silent".into()], "npm-test")
            } else {

                (
                    "npx",
                    vec![
                        "--no-install".into(),
                        "tsc".into(),
                        "--noEmit".into(),
                        "--pretty".into(),
                        "false".into(),
                    ],
                    "tsc-noemit",
                )
            };
            out.push(Box::new(TestRunnerVerifier {
                config: cfg.clone(),
                command: Some(CommandSpec {
                    program,
                    args,
                    parser: ParserKind::StderrTail,
                    cwd: root.clone(),
                    label,
                }),
            }));
        }

        if root.join("pyproject.toml").is_file() || root.join("setup.py").is_file() {
            let (program, args, label): (&'static str, Vec<String>, &'static str) = if heavy {
                (
                    "pytest",
                    vec!["-x".into(), "-q".into()],
                    "pytest-fast-fail",
                )
            } else {
                (
                    "pytest",
                    vec!["--collect-only".into(), "-q".into()],
                    "pytest-collect",
                )
            };
            out.push(Box::new(TestRunnerVerifier {
                config: cfg.clone(),
                command: Some(CommandSpec {
                    program,
                    args,
                    parser: ParserKind::StderrTail,
                    cwd: root.clone(),
                    label,
                }),
            }));
        }

        if root.join("go.mod").is_file() {
            let (program, args, label): (&'static str, Vec<String>, &'static str) = if heavy {
                ("go", vec!["test".into(), "./...".into()], "go-test")
            } else {
                ("go", vec!["vet".into(), "./...".into()], "go-vet")
            };
            out.push(Box::new(TestRunnerVerifier {
                config: cfg.clone(),
                command: Some(CommandSpec {
                    program,
                    args,
                    parser: ParserKind::StderrTail,
                    cwd: root,
                    label,
                }),
            }));
        }

        out
    }
}

pub struct TestRunnerVerifier {
    config: TestRunnerConfig,

    command: Option<CommandSpec>,
}

impl TestRunnerVerifier {
    pub fn new(config: TestRunnerConfig) -> Self {
        Self {
            config,
            command: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(TestRunnerConfig::default())
    }

    pub fn dry_run() -> Self {
        Self::new(TestRunnerConfig {
            dry_run: true,
            ..Default::default()
        })
    }
}

fn command_for(lang: Language) -> Option<(&'static str, &'static [&'static str])> {
    match lang {
        Language::Rust => Some(("cargo", &["check", "--quiet"] as &[&str])),
        Language::Python => Some(("pytest", &["-x", "-q"] as &[&str])),
        Language::TypeScript | Language::JavaScript => {
            Some(("npm", &["test", "--silent"] as &[&str]))
        }

        _ => None,
    }
}

fn resolve_cwd(cfg: &TestRunnerConfig, path: &Path) -> PathBuf {
    if let Some(o) = &cfg.cwd_override {
        return o.clone();
    }
    path.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[async_trait]
impl Verifier for TestRunnerVerifier {
    fn name(&self) -> &'static str {
        "test_runner"
    }

    async fn verify(&self, artifact: &Artifact) -> anyhow::Result<VerificationReport> {
        if self.config.dry_run {
            return Ok(VerificationReport::ok(self.name()));
        }

        let is_workspace =
            matches!(artifact.kind, super::traits::ArtifactKind::Workspace);
        if is_workspace && self.command.is_none() {
            return Ok(VerificationReport::ok(self.name()));
        }

        let (program, args, parser, cwd, label) = if let Some(spec) = &self.command {
            (
                spec.program.to_string(),
                spec.args.clone(),
                spec.parser,
                spec.cwd.clone(),
                spec.label,
            )
        } else {
            let Some((program, args)) = command_for(artifact.language) else {
                return Ok(VerificationReport::ok(self.name()));
            };
            (
                program.to_string(),
                args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
                ParserKind::StderrTail,
                resolve_cwd(&self.config, &artifact.path),
                "single-file",
            )
        };

        let mut cmd = crate::util::hidden_async_command(&program);
        cmd.args(&args)
            .current_dir(&cwd)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let collect = async {
            use tokio::io::AsyncReadExt;
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();
            let read_out = async {
                if let Some(pipe) = stdout_pipe.as_mut() {
                    let _ = pipe.read_to_end(&mut stdout_buf).await;
                }
            };
            let read_err = async {
                if let Some(pipe) = stderr_pipe.as_mut() {
                    let _ = pipe.read_to_end(&mut stderr_buf).await;
                }
            };
            let (status, (), ()) = tokio::join!(child.wait(), read_out, read_err);
            (status, stdout_buf, stderr_buf)
        };
        let output = match timeout(self.config.timeout, collect).await {
            Ok((Ok(status), stdout, stderr)) => std::process::Output {
                status,
                stdout,
                stderr,
            },
            Ok((Err(e), _, _)) => {
                return Ok(VerificationReport::failed(
                    self.name(),
                    vec![],
                    format!("status=child-error label={label} err={e}"),
                ));
            }
            Err(_) => {
                let _ = child.start_kill();
                let _ = timeout(Duration::from_secs(3), child.wait()).await;
                let secs = self.config.timeout.as_secs();
                return Ok(VerificationReport {
                    verifier: self.name(),
                    passed: false,
                    issues: vec![VerificationIssue {
                        line: 0,
                        column: 0,
                        message: format!(
                            "test_runner timed out after {secs}s ({label}); result is UNKNOWN, \
                             not a pass - re-run verification or raise the timeout"
                        ),
                        severity: IssueSeverity::Warning,
                    }],
                    summary: format!("status=timeout(unknown) label={label} timeout_secs={secs}"),
                });
            }
        };

        let exit_ok = output.status.success();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

        match parser {
            ParserKind::CargoJson => {
                let (issues, truncated) =
                    parse_cargo_json(&stdout, self.config.max_diagnostics);
                let error_count = issues
                    .iter()
                    .filter(|i| matches!(i.severity, IssueSeverity::Error))
                    .count();
                let summary = if truncated > 0 {
                    format!(
                        "label={label} exit={} errors={error_count} truncated={truncated}",
                        output.status
                    )
                } else {
                    format!(
                        "label={label} exit={} errors={error_count}",
                        output.status
                    )
                };
                let passed = exit_ok && error_count == 0;
                Ok(if passed {
                    VerificationReport {
                        verifier: self.name(),
                        passed: true,
                        issues,
                        summary,
                    }
                } else {
                    VerificationReport::failed(self.name(), issues, summary)
                })
            }
            ParserKind::StderrTail => {
                let summary = if stderr.len() > self.config.stderr_tail_chars {
                    let start = crate::util::ceil_char_boundary(
                        &stderr,
                        stderr.len() - self.config.stderr_tail_chars,
                    );
                    format!("label={label} ...{}", &stderr[start..])
                } else {
                    format!("label={label} {stderr}")
                };
                Ok(if exit_ok {
                    VerificationReport {
                        verifier: self.name(),
                        passed: true,
                        issues: vec![],
                        summary,
                    }
                } else {
                    VerificationReport::failed(
                        self.name(),
                        vec![VerificationIssue {
                            line: 0,
                            column: 0,
                            message: format!("{program} exited with {}", output.status),
                            severity: IssueSeverity::Error,
                        }],
                        summary,
                    )
                })
            }
        }
    }
}

fn parse_cargo_json(stdout: &str, max: usize) -> (Vec<VerificationIssue>, usize) {
    let mut out: Vec<VerificationIssue> = Vec::new();
    let mut truncated = 0usize;

    for raw in stdout.lines() {
        if raw.is_empty() {
            continue;
        }
        let v: JsonValue = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("reason").and_then(JsonValue::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        let level = msg
            .get("level")
            .and_then(JsonValue::as_str)
            .unwrap_or("note");
        let severity = match level {
            "error" | "error[E0...]" | "ice" => IssueSeverity::Error,
            "warning" => IssueSeverity::Warning,
            _ => IssueSeverity::Info,
        };

        if matches!(severity, IssueSeverity::Info) {
            continue;
        }
        let rendered = msg
            .get("rendered")
            .and_then(JsonValue::as_str)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let primary_span = msg
            .get("spans")
            .and_then(JsonValue::as_array)
            .and_then(|spans| {
                spans
                    .iter()
                    .find(|s| s.get("is_primary").and_then(JsonValue::as_bool) == Some(true))
            });
        let (line, column) = primary_span
            .map(|s| {
                let l = s
                    .get("line_start")
                    .and_then(JsonValue::as_u64)
                    .unwrap_or(0) as u32;
                let c = s
                    .get("column_start")
                    .and_then(JsonValue::as_u64)
                    .unwrap_or(0) as u32;
                (l, c)
            })
            .unwrap_or((0, 0));
        let head = if rendered.is_empty() {
            msg.get("message")
                .and_then(JsonValue::as_str)
                .unwrap_or("compile error")
                .to_string()
        } else {

            rendered.lines().next().unwrap_or(&rendered).to_string()
        };
        if out.len() >= max {
            truncated += 1;
            continue;
        }
        out.push(VerificationIssue {
            line,
            column,
            message: head,
            severity,
        });
    }

    (out, truncated)
}
