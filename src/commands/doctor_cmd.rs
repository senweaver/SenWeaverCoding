// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /doctor command — mirrors claude-code-typescript-src`commands/doctor/`.
// Runs diagnostic checks on the agent environment.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let subcommand = ctx.args.first().map(|s| s.as_str()).unwrap_or("");
    if subcommand == "network" {
        return handle_network().await;
    }

    let mut checks = Vec::new();

    checks.push(format!(
        "  OS: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    checks.push("  Rust edition: 2024".to_string());

    match std::process::Command::new("git").arg("--version").output() {
        Ok(out) => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            checks.push(format!("  ✓ git: {ver}"));
        }
        Err(_) => {
            checks.push("  ✗ git: NOT FOUND (required for version control)".to_string());
        }
    }

    match std::process::Command::new("node").arg("--version").output() {
        Ok(out) => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            checks.push(format!("  ✓ node: {ver}"));
        }
        Err(_) => {
            checks.push("  ⚠ node: not found (optional, needed for MCP servers)".to_string());
        }
    }

    match std::process::Command::new("python3")
        .arg("--version")
        .output()
    {
        Ok(out) => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            checks.push(format!("  ✓ python: {ver}"));
        }
        Err(_) => match std::process::Command::new("python")
            .arg("--version")
            .output()
        {
            Ok(out) => {
                let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                checks.push(format!("  ✓ python: {ver}"));
            }
            Err(_) => checks.push("  ⚠ python: not found (optional)".to_string()),
        },
    }

    match std::process::Command::new("rg").arg("--version").output() {
        Ok(out) => {
            let ver = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            checks.push(format!("  ✓ ripgrep: {ver}"));
        }
        Err(_) => {
            checks.push("  ⚠ rg: not found (recommended for fast search)".to_string());
        }
    }

    match crate::config::Config::load_or_init().await {
        Ok(cfg) => {
            let provider = cfg.default_provider.as_deref().unwrap_or("(not set)");
            checks.push(format!("  ✓ config: OK (provider: {provider})"));

            let has_key = cfg.api_key.as_ref().map_or(false, |k| !k.is_empty());
            let env_key = std::env::var("ANTHROPIC_API_KEY").is_ok()
                || std::env::var("OPENAI_API_KEY").is_ok()
                || std::env::var("GEMINI_API_KEY").is_ok()
                || std::env::var("SEN_API_KEY").is_ok();
            if has_key || env_key {
                checks.push("  ✓ API key: configured".to_string());
            } else {
                checks.push(
                    "  ✗ API key: NOT FOUND — set via config or environment variable".to_string(),
                );
            }
        }
        Err(e) => checks.push(format!("  ✗ config: ERROR ({e})")),
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let has_git = cwd.join(".git").exists();
    checks.push(format!(
        "  {} workspace: {} (git: {})",
        if has_git { "✓" } else { "⚠" },
        cwd.display(),
        if has_git {
            "yes"
        } else {
            "no — consider running git init"
        }
    ));

    let sen_dir = cwd.join(".senweavercoding");
    if sen_dir.exists() {
        checks.push("  ✓ .senweavercoding dir: exists".to_string());
    } else {
        checks.push(
            "  ⚠ .senweavercoding dir: not found (will be created on first use)".to_string(),
        );
    }

    match std::panic::catch_unwind(crate::bootstrap::get_state) {
        Ok(bs) => {
            let mut sid = String::new();
            let mut cost = 0.0f64;
            bs.read(|state| {
                sid = state.session_id.to_string();
                cost = state.total_cost_usd;
            });
            checks.push(format!("  ✓ session: {sid} (cost: ${cost:.4})"));
        }
        Err(_) => {
            checks.push(
                "  ⚠ bootstrap: not initialized (normal on first command)".to_string(),
            );
        }
    }

    let mut lines = vec!["Diagnostics:".to_string()];
    lines.extend(checks);
    lines.push(String::new());

    let error_count = lines.iter().filter(|l| l.contains('✗')).count();
    let warn_count = lines.iter().filter(|l| l.contains('⚠')).count();
    if error_count > 0 {
        lines.push(format!(
            "{error_count} error(s), {warn_count} warning(s) — fix errors above."
        ));
    } else if warn_count > 0 {
        lines.push(format!(
            "All checks passed with {warn_count} warning(s)."
        ));
    } else {
        lines.push("All checks passed! Environment is ready.".to_string());
    }

    CommandResult::ok(lines.join("\n"))
}

async fn handle_network() -> CommandResult {
    let mut lines = vec!["Network Search Channel Diagnostics:".to_string()];
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (compatible; SenWeaverCoding/1.0)")
        .build()
        .unwrap_or_default();

    // DuckDuckGo
    match client
        .get("https://html.duckduckgo.com/html/?q=test")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            lines.push("  [OK]   DuckDuckGo — reachable".to_string());
        }
        Ok(r) => {
            lines.push(format!(
                "  [FAIL] DuckDuckGo — HTTP {}",
                r.status()
            ));
        }
        Err(e) => {
            lines.push(format!("  [FAIL] DuckDuckGo — {e}"));
        }
    }

    // Brave
    let brave_key = std::env::var("BRAVE_API_KEY").ok().or_else(|| {
        if let Ok(cfg) = tokio::runtime::Handle::current()
            .block_on(crate::config::Config::load_or_init())
        {
            cfg.web_search.brave_api_key.clone()
        } else {
            None
        }
    });
    match brave_key {
        Some(ref key) if !key.is_empty() => {
            match client
                .get("https://api.search.brave.com/res/v1/web/search?q=test&count=1")
                .header("X-Subscription-Token", key.as_str())
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    lines.push("  [OK]   Brave Search — API key valid".to_string());
                }
                Ok(r) => {
                    lines.push(format!(
                        "  [FAIL] Brave Search — HTTP {} (check API key)",
                        r.status()
                    ));
                }
                Err(e) => {
                    lines.push(format!("  [FAIL] Brave Search — {e}"));
                }
            }
        }
        _ => {
            lines.push("  [SKIP] Brave Search — no API key configured".to_string());
        }
    }

    // SearXNG
    let searxng_url = std::env::var("SEARXNG_INSTANCE_URL").ok().or_else(|| {
        if let Ok(cfg) = tokio::runtime::Handle::current()
            .block_on(crate::config::Config::load_or_init())
        {
            cfg.web_search.searxng_instance_url.clone()
        } else {
            None
        }
    });
    match searxng_url {
        Some(ref url) if !url.is_empty() => {
            match client
                .get(format!("{url}/search?q=test&format=json"))
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    lines.push("  [OK]   SearXNG — instance reachable".to_string());
                }
                Ok(r) => {
                    lines.push(format!(
                        "  [FAIL] SearXNG — HTTP {}",
                        r.status()
                    ));
                }
                Err(e) => {
                    lines.push(format!("  [FAIL] SearXNG — {e}"));
                }
            }
        }
        _ => {
            lines.push("  [SKIP] SearXNG — no instance URL configured".to_string());
        }
    }

    // Tavily
    match std::env::var("TAVILY_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    {
        Some(ref key) => {
            let body = serde_json::json!({
                "api_key": key,
                "query": "test",
                "max_results": 1,
            });
            match client
                .post("https://api.tavily.com/search")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    lines.push("  [OK]   Tavily — API key valid".to_string());
                }
                Ok(r) => {
                    lines.push(format!(
                        "  [FAIL] Tavily — HTTP {} (check API key)",
                        r.status()
                    ));
                }
                Err(e) => {
                    lines.push(format!("  [FAIL] Tavily — {e}"));
                }
            }
        }
        None => {
            lines.push("  [SKIP] Tavily — no API key (set TAVILY_API_KEY)".to_string());
        }
    }

    // Exa
    match std::env::var("EXA_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    {
        Some(ref key) => {
            let body = serde_json::json!({
                "query": "test",
                "numResults": 1,
            });
            match client
                .post("https://api.exa.ai/search")
                .header("x-api-key", key.as_str())
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    lines.push("  [OK]   Exa — API key valid".to_string());
                }
                Ok(r) => {
                    lines.push(format!(
                        "  [FAIL] Exa — HTTP {} (check API key)",
                        r.status()
                    ));
                }
                Err(e) => {
                    lines.push(format!("  [FAIL] Exa — {e}"));
                }
            }
        }
        None => {
            lines.push("  [SKIP] Exa — no API key (set EXA_API_KEY)".to_string());
        }
    }

    let ok_count = lines.iter().filter(|l| l.contains("[OK]")).count();
    let fail_count = lines.iter().filter(|l| l.contains("[FAIL]")).count();
    let skip_count = lines.iter().filter(|l| l.contains("[SKIP]")).count();
    lines.push(String::new());
    lines.push(format!(
        "{ok_count} OK, {fail_count} failed, {skip_count} skipped"
    ));

    CommandResult::ok(lines.join("\n"))
}
