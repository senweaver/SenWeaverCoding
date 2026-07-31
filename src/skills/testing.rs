// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use anyhow::{Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};
const TEST_FILE_NAME: &str = "TEST.sh";

#[derive(Debug, Clone)]
pub struct SkillTestResult {
    pub skill_name: String,
    pub tests_run: usize,
    pub tests_passed: usize,
    pub failures: Vec<TestFailure>,
}

#[derive(Debug, Clone)]
pub struct TestFailure {
    pub command: String,
    pub expected_exit: i32,
    pub actual_exit: i32,
    pub expected_pattern: String,
    pub actual_output: String,
}

#[derive(Debug, Clone)]
struct TestCase {
    command: String,
    expected_exit: i32,
    expected_pattern: String,
}

fn parse_test_line(line: &str) -> Option<TestCase> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let parts: Vec<&str> = trimmed.split(" | ").collect();
    if parts.len() < 3 {

        let parts: Vec<&str> = trimmed.splitn(3, '|').collect();
        if parts.len() < 3 {
            return None;
        }
        let command = parts[0].trim().to_string();
        let expected_exit = parts[1].trim().parse::<i32>().ok()?;
        let expected_pattern = parts[2].trim().to_string();
        return Some(TestCase {
            command,
            expected_exit,
            expected_pattern,
        });
    }

    let command = parts[0].trim().to_string();
    let expected_exit = parts[1].trim().parse::<i32>().ok()?;

    let expected_pattern = parts[2..].join(" | ").trim().to_string();

    Some(TestCase {
        command,
        expected_exit,
        expected_pattern,
    })
}

fn pattern_matches(output: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    if let Ok(re) = Regex::new(pattern) {
        if re.is_match(output) {
            return true;
        }
    }

    output.contains(pattern)
}

fn run_test_case(case: &TestCase, skill_dir: &Path, verbose: bool) -> Option<TestFailure> {
    if verbose {
        println!("    running: {}", case.command);
    }

    let result = crate::util::hidden_sync_command("sh")
        .arg("-c")
        .arg(&case.command)
        .current_dir(skill_dir)
        .output();

    let output = match result {
        Ok(o) => o,
        Err(err) => {
            return Some(TestFailure {
                command: case.command.clone(),
                expected_exit: case.expected_exit,
                actual_exit: -1,
                expected_pattern: case.expected_pattern.clone(),
                actual_output: format!("failed to execute command: {err}"),
            });
        }
    };

    let actual_exit = output.status.code().unwrap_or(-1);
    let stdout = crate::util::decode_subprocess_bytes(&output.stdout);
    let stderr = crate::util::decode_subprocess_bytes(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    if verbose {
        if !stdout.is_empty() {
            println!("    stdout: {}", stdout.trim());
        }
        if !stderr.is_empty() {
            println!("    stderr: {}", stderr.trim());
        }
        println!("    exit: {actual_exit}");
    }

    let exit_ok = actual_exit == case.expected_exit;
    let pattern_ok = pattern_matches(&combined, &case.expected_pattern);

    if exit_ok && pattern_ok {
        None
    } else {
        Some(TestFailure {
            command: case.command.clone(),
            expected_exit: case.expected_exit,
            actual_exit,
            expected_pattern: case.expected_pattern.clone(),
            actual_output: combined.to_string(),
        })
    }
}

pub fn test_skill(skill_dir: &Path, skill_name: &str, verbose: bool) -> Result<SkillTestResult> {
    let test_file = skill_dir.join(TEST_FILE_NAME);
    if !test_file.exists() {
        return Ok(SkillTestResult {
            skill_name: skill_name.to_string(),
            tests_run: 0,
            tests_passed: 0,
            failures: Vec::new(),
        });
    }

    let content = std::fs::read_to_string(&test_file)
        .with_context(|| format!("failed to read {}", test_file.display()))?;

    let cases: Vec<TestCase> = content.lines().filter_map(parse_test_line).collect();

    let mut result = SkillTestResult {
        skill_name: skill_name.to_string(),
        tests_run: cases.len(),
        tests_passed: 0,
        failures: Vec::new(),
    };

    for case in &cases {
        match run_test_case(case, skill_dir, verbose) {
            None => result.tests_passed += 1,
            Some(failure) => result.failures.push(failure),
        }
    }

    Ok(result)
}

pub fn test_all_skills(skills_dirs: &[PathBuf], verbose: bool) -> Result<Vec<SkillTestResult>> {
    let mut results = Vec::new();

    for dir in skills_dirs {
        if !dir.exists() || !dir.is_dir() {
            continue;
        }

        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let test_file = path.join(TEST_FILE_NAME);
            if !test_file.exists() {
                continue;
            }
            let skill_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if verbose {
                println!("  Testing skill: {} ({})", skill_name, path.display());
            }

            let r = test_skill(&path, &skill_name, verbose)?;
            results.push(r);
        }
    }

    Ok(results)
}

pub fn print_results(results: &[SkillTestResult]) {
    if results.is_empty() {
        println!("No skills with {} found.", TEST_FILE_NAME);
        return;
    }

    println!();
    for r in results {
        if r.tests_run == 0 {
            println!(
                "  {} {}  -  no test cases",
                console::style("-").dim(),
                r.skill_name,
            );
            continue;
        }

        if r.failures.is_empty() {
            println!(
                "  {} {}  -  {}/{} passed",
                console::style("✓").green().bold(),
                console::style(&r.skill_name).white().bold(),
                r.tests_passed,
                r.tests_run,
            );
        } else {
            println!(
                "  {} {}  -  {}/{} passed",
                console::style("✗").red().bold(),
                console::style(&r.skill_name).white().bold(),
                r.tests_passed,
                r.tests_run,
            );
            for f in &r.failures {
                println!("    command:  {}", console::style(&f.command).dim(),);
                println!(
                    "    expected: exit={}, pattern={}",
                    f.expected_exit, f.expected_pattern,
                );
                println!(
                    "    actual:   exit={}, output={}",
                    f.actual_exit,
                    truncate_output(&f.actual_output, 200),
                );
                println!();
            }
        }
    }

    let total_run: usize = results.iter().map(|r| r.tests_run).sum();
    let total_passed: usize = results.iter().map(|r| r.tests_passed).sum();
    let total_failed = total_run - total_passed;

    println!();
    if total_failed == 0 {
        println!(
            "  {} All {total_run} test(s) passed across {} skill(s).",
            console::style("✓").green().bold(),
            results.len(),
        );
    } else {
        println!(
            "  {} {total_failed} of {total_run} test(s) failed across {} skill(s).",
            console::style("✗").red().bold(),
            results.len(),
        );
    }
    println!();
}

fn truncate_output(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max {
        trimmed.replace('\n', " ")
    } else {
        let mut end = max;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &trimmed[..end].replace('\n', " "))
    }
}
