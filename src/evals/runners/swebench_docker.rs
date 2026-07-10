// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::evals::runner::EvalExecutor;
use crate::evals::traits::{EvalProblem, EvalSuite, ProblemResult, Verdict};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweBenchInstance {
    pub instance_id: String,
    pub repo: String,
    pub base_commit: String,
    #[serde(default)]
    pub problem_statement: String,

    #[serde(deserialize_with = "deserialize_test_list", default)]
    pub fail_to_pass: Vec<String>,

    #[serde(deserialize_with = "deserialize_test_list", default)]
    pub pass_to_pass: Vec<String>,

    #[serde(default)]
    pub test_patch: String,

    #[serde(default)]
    pub environment_image: Option<String>,

    #[serde(default)]
    pub patch: Option<String>,
}

fn deserialize_test_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect()),
        serde_json::Value::String(s) => Ok(s
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()),
        serde_json::Value::Null => Ok(Vec::new()),
        other => Err(D::Error::custom(format!(
            "expected array or string for test list, got {other:?}"
        ))),
    }
}

impl SweBenchInstance {

    pub fn default_image_tag(&self) -> String {

        let normalised = self.instance_id.replace('/', "_").to_lowercase();
        format!("sweb.eval.x86_64.{normalised}:latest")
    }
}

#[derive(Debug, Clone)]
pub struct SweBenchDockerSuite {
    instances: Vec<SweBenchInstance>,

    by_id: HashMap<String, SweBenchInstance>,
}

impl SweBenchDockerSuite {

    pub fn from_instances(instances: Vec<SweBenchInstance>) -> Self {
        let by_id = instances
            .iter()
            .map(|i| (i.instance_id.clone(), i.clone()))
            .collect();
        Self { instances, by_id }
    }

    pub fn from_jsonl(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        let mut instances = Vec::new();
        for (lineno, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let inst: SweBenchInstance = serde_json::from_str(line).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("swebench line {}: {e}", lineno + 1),
                )
            })?;
            instances.push(inst);
        }
        Ok(Self::from_instances(instances))
    }

    pub fn to_problems(&self) -> Vec<EvalProblem> {
        self.instances.iter().map(instance_to_problem).collect()
    }
}

fn instance_to_problem(inst: &SweBenchInstance) -> EvalProblem {
    let mut metadata = HashMap::new();
    metadata.insert("repo".to_string(), inst.repo.clone());
    metadata.insert("base_commit".to_string(), inst.base_commit.clone());
    metadata.insert("fail_to_pass".to_string(), inst.fail_to_pass.join("\n"));
    metadata.insert("pass_to_pass".to_string(), inst.pass_to_pass.join("\n"));
    if let Some(img) = &inst.environment_image {
        metadata.insert("environment_image".to_string(), img.clone());
    }
    EvalProblem {
        id: inst.instance_id.clone(),
        prompt: inst.problem_statement.clone(),
        reference: inst.patch.clone(),
        metadata,
    }
}

#[async_trait]
impl EvalSuite for SweBenchDockerSuite {
    fn name(&self) -> &'static str {
        "swebench-docker"
    }

    async fn problems(&self) -> Vec<EvalProblem> {
        self.to_problems()
    }

    async fn judge(&self, problem: &EvalProblem, output: &str) -> ProblemResult {
        let report: SweBenchExecutorReport = match serde_json::from_str(output) {
            Ok(r) => r,
            Err(e) => {
                return ProblemResult {
                    problem_id: problem.id.clone(),
                    verdict: Verdict::Error,
                    output: output.to_string(),
                    latency_ms: 0,
                    notes: Some(format!("invalid executor report: {e}")),
                };
            }
        };

        let inst = self.by_id.get(&problem.id);
        let fail_to_pass: HashSet<&str> = inst
            .map(|i| i.fail_to_pass.iter().map(String::as_str).collect())
            .unwrap_or_default();
        let pass_to_pass: HashSet<&str> = inst
            .map(|i| i.pass_to_pass.iter().map(String::as_str).collect())
            .unwrap_or_default();

        let resolved: HashSet<&str> = report.tests.resolved.iter().map(String::as_str).collect();
        let unresolved: HashSet<&str> =
            report.tests.unresolved.iter().map(String::as_str).collect();
        let regressions: HashSet<&str> = report.tests.regressions.iter().map(String::as_str).collect();

        let f2p_resolved = fail_to_pass.iter().all(|t| resolved.contains(t));
        let p2p_intact = pass_to_pass.iter().all(|t| !regressions.contains(t));
        let no_unresolved_in_target = fail_to_pass.iter().all(|t| !unresolved.contains(t));

        let verdict = if f2p_resolved && p2p_intact && no_unresolved_in_target {
            Verdict::Pass
        } else if !report.applied {
            Verdict::Error
        } else {
            Verdict::Fail
        };

        ProblemResult {
            problem_id: problem.id.clone(),
            verdict,
            output: output.to_string(),
            latency_ms: 0,
            notes: Some(format!(
                "applied={} resolved={}/{} regressions={} runtime_ms={}",
                report.applied,
                resolved.len(),
                fail_to_pass.len(),
                regressions.len(),
                report.runtime_ms,
            )),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SweBenchExecutorReport {

    #[serde(default)]
    pub applied: bool,

    #[serde(default)]
    pub runtime_ms: u64,

    #[serde(default)]
    pub tests: SweBenchTestSummary,

    #[serde(default)]
    pub log_excerpt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SweBenchTestSummary {

    #[serde(default)]
    pub resolved: Vec<String>,

    #[serde(default)]
    pub unresolved: Vec<String>,

    #[serde(default)]
    pub regressions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SweBenchDockerConfig {

    pub docker_bin: PathBuf,

    pub image_template: Option<String>,

    pub timeout: Duration,

    pub extra_run_args: Vec<String>,

    pub workdir: PathBuf,
}

impl Default for SweBenchDockerConfig {
    fn default() -> Self {
        Self {
            docker_bin: PathBuf::from("docker"),
            image_template: None,
            timeout: Duration::from_secs(15 * 60),
            extra_run_args: Vec::new(),
            workdir: std::env::temp_dir(),
        }
    }
}

pub struct SweBenchDockerExecutor {
    inner: Arc<dyn EvalExecutor>,
    suite: Arc<SweBenchDockerSuite>,
    config: SweBenchDockerConfig,
}

impl SweBenchDockerExecutor {
    pub fn new(
        inner: Arc<dyn EvalExecutor>,
        suite: Arc<SweBenchDockerSuite>,
        config: SweBenchDockerConfig,
    ) -> Self {
        Self {
            inner,
            suite,
            config,
        }
    }

    fn resolve_image(&self, inst: &SweBenchInstance) -> String {
        if let Some(img) = &inst.environment_image {
            return img.clone();
        }
        if let Some(tmpl) = &self.config.image_template {
            let normalised = inst.instance_id.replace('/', "_").to_lowercase();
            return tmpl.replace("{instance}", &normalised);
        }
        inst.default_image_tag()
    }

    fn stage_patches(
        &self,
        inst: &SweBenchInstance,
        model_patch: &str,
    ) -> std::io::Result<PathBuf> {
        let dir = self
            .config
            .workdir
            .join(format!("swebench-{}", sanitise_for_path(&inst.instance_id)));

        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("model.patch"), model_patch)?;
        std::fs::write(dir.join("test.patch"), &inst.test_patch)?;

        let placeholder = serde_json::to_string(&SweBenchExecutorReport {
            applied: false,
            runtime_ms: 0,
            tests: SweBenchTestSummary::default(),
            log_excerpt: Some("container did not produce a report".to_string()),
        })
        .unwrap_or_else(|_| "{}".to_string());
        std::fs::write(dir.join("report.json"), placeholder)?;
        Ok(dir)
    }

    fn build_docker_args(&self, image: &str, patch_dir: &Path, container_name: &str) -> Vec<String> {

        let mount = format!(
            "{}:/patches",
            patch_dir.to_string_lossy()
        );
        let mut args: Vec<String> = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            container_name.to_string(),
            "-v".to_string(),
            mount,
        ];
        args.extend(self.config.extra_run_args.iter().cloned());
        args.push(image.to_string());

        args.push("/eval/run_swebench.sh".to_string());
        args
    }

    pub async fn execute_grading(
        &self,
        inst: &SweBenchInstance,
        model_patch: &str,
    ) -> anyhow::Result<SweBenchExecutorReport> {
        let started = std::time::Instant::now();
        let patch_dir = self.stage_patches(inst, model_patch)?;
        let image = self.resolve_image(inst);
        let container_name = format!(
            "swebench-{}-{}",
            sanitise_for_path(&inst.instance_id),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let args = self.build_docker_args(&image, &patch_dir, &container_name);

        let mut cmd = crate::util::hidden_async_command(&self.config.docker_bin);
        cmd.args(&args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);

        let timeout = self.config.timeout;
        let run_future = cmd.output();
        let output = match tokio::time::timeout(timeout, run_future).await {
            Ok(res) => res?,
            Err(_) => {
                let mut kill_cmd = crate::util::hidden_async_command(&self.config.docker_bin);
                kill_cmd.args(["kill", &container_name]);
                kill_cmd.stdout(std::process::Stdio::null());
                kill_cmd.stderr(std::process::Stdio::null());
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    kill_cmd.status(),
                )
                .await;
                anyhow::bail!(
                    "swebench-docker timeout after {}s for instance {}; container {} killed",
                    timeout.as_secs(),
                    inst.instance_id,
                    container_name
                );
            }
        };

        let report_path = patch_dir.join("report.json");
        let raw = std::fs::read_to_string(&report_path).unwrap_or_else(|_| {

            serde_json::to_string(&SweBenchExecutorReport {
                applied: false,
                runtime_ms: started.elapsed().as_millis() as u64,
                tests: SweBenchTestSummary::default(),
                log_excerpt: Some(format!(
                    "container exited {} but no report.json was produced",
                    output.status
                )),
            })
            .unwrap_or_else(|_| "{}".to_string())
        });
        let mut report: SweBenchExecutorReport =
            serde_json::from_str(&raw).unwrap_or_default();
        if report.runtime_ms == 0 {
            report.runtime_ms = started.elapsed().as_millis() as u64;
        }
        if report.log_excerpt.is_none() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            report.log_excerpt = Some(truncate(&stderr, 4096));
        }
        Ok(report)
    }
}

#[async_trait]
impl EvalExecutor for SweBenchDockerExecutor {
    async fn run(&self, problem: &EvalProblem) -> Result<String, anyhow::Error> {

        let inst = self
            .suite
            .by_id
            .get(&problem.id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("instance {} not found in suite", problem.id))?;

        let model_patch = self.inner.run(problem).await?;

        let report = self.execute_grading(&inst, &model_patch).await?;
        Ok(serde_json::to_string(&report)?)
    }
}

fn sanitise_for_path(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = crate::util::truncate_str_bytes(s, max).to_string();
    out.push_str("\n...[truncated]...");
    out
}
