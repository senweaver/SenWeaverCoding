// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::engine::{SearchContext, SearchHit};
use super::super::super::parsers::clean_text;
use serde_json::Value;

pub async fn github_api_get(
    ctx: &SearchContext,
    endpoint: &str,
    extra_accept: Option<&str>,
) -> anyhow::Result<Value> {
    let mut sort = ctx.extra_str("sort").map(|s| s.to_string());
    let order = ctx
        .extra_str("order")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "desc".to_string());
    let composed = compose_advanced_query(ctx);
    let per_page = ctx.limit.clamp(5, 50);
    let mut url = format!(
        "https://api.github.com/search/{endpoint}?q={}&per_page={per_page}&order={order}",
        urlencoding::encode(&composed)
    );
    if let Some(s) = sort.take() {
        if !s.is_empty() {
            url.push_str(&format!("&sort={}", urlencoding::encode(&s)));
        }
    }
    let client = ctx.build_http_client()?;
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "SenWeaverCoding/1.0");
    if let Some(accept) = extra_accept {
        req = req.header("Accept", accept);
    }
    if let Some(token) = ctx.api_keys.github_token.as_ref().filter(|t| !t.is_empty()) {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let response = req.send().await?;
    if response.status().as_u16() == 403 {
        anyhow::bail!(
            "GitHub API rate limit exceeded (set GITHUB_TOKEN env var to increase quota)"
        );
    }
    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub {endpoint} search failed: HTTP {}",
            response.status()
        );
    }
    let body: Value = response.json().await?;
    Ok(body)
}

pub fn items_array(body: &Value) -> Vec<Value> {
    body.get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

pub fn build_repo_hit(engine_id: &'static str, item: &Value) -> Option<SearchHit> {
    let full_name = item
        .get("full_name")
        .and_then(|v| v.as_str())
        .map(clean_text)?;
    let url = item
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let desc = item
        .get("description")
        .and_then(|v| v.as_str())
        .map(clean_text)
        .unwrap_or_default();
    let stars = item
        .get("stargazers_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let forks = item.get("forks_count").and_then(|v| v.as_i64()).unwrap_or(0);
    let lang = item
        .get("language")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let updated = item
        .get("updated_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let license = item
        .get("license")
        .and_then(|v| v.get("spdx_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let mut bits: Vec<String> = vec![format!("⭐{stars}"), format!("🔱 {forks}")];
    if !lang.is_empty() {
        bits.push(lang);
    }
    if !license.is_empty() {
        bits.push(format!("license:{license}"));
    }
    let source = format!("GitHub · {}", bits.join(" · "));
    let mut hit = SearchHit::new(engine_id, full_name, url)
        .with_description(desc)
        .with_source(source);
    if let Some(u) = updated {
        hit = hit.with_published_at(u);
    }
    Some(hit)
}

pub fn build_code_hit(engine_id: &'static str, item: &Value) -> Option<SearchHit> {
    let path = item
        .get("path")
        .and_then(|v| v.as_str())
        .map(clean_text)?;
    let url = item
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let repo = item
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|v| v.as_str())
        .map(clean_text)
        .unwrap_or_default();
    let title = if repo.is_empty() {
        path.clone()
    } else {
        format!("{repo}  - {path}")
    };
    let mut snippet = String::new();
    if let Some(matches) = item.get("text_matches").and_then(|v| v.as_array()) {
        for m in matches.iter().take(2) {
            if let Some(fragment) = m.get("fragment").and_then(|v| v.as_str()) {
                if !snippet.is_empty() {
                    snippet.push_str(" / ");
                }
                snippet.push_str(&fragment.replace('\n', " "));
            }
        }
    }
    let source = if repo.is_empty() {
        "GitHub code".to_string()
    } else {
        format!("{repo}  - GitHub code")
    };
    Some(
        SearchHit::new(engine_id, title, url)
            .with_description(snippet)
            .with_source(source),
    )
}

pub fn build_issue_hit(engine_id: &'static str, item: &Value) -> Option<SearchHit> {
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .map(clean_text)?;
    let url = item
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let body = item
        .get("body")
        .and_then(|v| v.as_str())
        .map(clean_text)
        .unwrap_or_default();
    let snippet = if body.chars().count() > 240 {
        let truncated: String = body.chars().take(240).collect();
        format!("{truncated}...")
    } else {
        body
    };
    let state = item
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let comments = item.get("comments").and_then(|v| v.as_i64()).unwrap_or(0);
    let user = item
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let is_pr = item.get("pull_request").is_some();
    let kind = if is_pr { "PR" } else { "Issue" };
    let bits = [
        format!("[{kind}]"),
        format!("state:{state}"),
        format!("💬 {comments}"),
    ];
    let source = if user.is_empty() {
        format!("GitHub · {}", bits.join(" "))
    } else {
        format!("{user}  - GitHub · {}", bits.join(" "))
    };
    let created = item
        .get("created_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut hit = SearchHit::new(engine_id, title, url)
        .with_description(snippet)
        .with_source(source);
    if let Some(c) = created {
        hit = hit.with_published_at(c);
    }
    Some(hit)
}

pub fn build_user_hit(engine_id: &'static str, item: &Value) -> Option<SearchHit> {
    let login = item
        .get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let url = item
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let kind = item
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "User".to_string());
    let source = format!("GitHub · {kind}");
    Some(SearchHit::new(engine_id, login, url).with_source(source))
}

pub fn compose_advanced_query(ctx: &SearchContext) -> String {
    let mut parts: Vec<String> = Vec::new();
    let raw = ctx.query.trim();
    if !raw.is_empty() {
        parts.push(raw.to_string());
    }
    let extra = &ctx.extra;
    push_array_qualifier(&mut parts, extra.get("owners"), |v| {
        let t = v.trim();
        if t.is_empty() {
            return None;
        }
        if t.starts_with("user:") || t.starts_with("org:") {
            return Some(t.to_string());
        }
        Some(format!("user:{t}"))
    });
    push_array_qualifier(&mut parts, extra.get("repos"), |v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(format!("repo:{t}"))
        }
    });
    push_array_qualifier(&mut parts, extra.get("languages"), |v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(format!("language:{t}"))
        }
    });
    push_array_qualifier(&mut parts, extra.get("topics"), |v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(format!("topic:{t}"))
        }
    });
    push_array_qualifier(&mut parts, extra.get("labels"), |v| {
        let t = v.trim();
        if t.is_empty() {
            return None;
        }
        if t.contains(' ') {
            Some(format!("label:\"{t}\""))
        } else {
            Some(format!("label:{t}"))
        }
    });
    push_array_qualifier(&mut parts, extra.get("in_fields"), |v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(format!("in:{t}"))
        }
    });
    push_string_qualifier(&mut parts, extra.get("license"), "license");
    push_string_qualifier(&mut parts, extra.get("stars"), "stars");
    push_string_qualifier(&mut parts, extra.get("forks"), "forks");
    push_string_qualifier(&mut parts, extra.get("size_kb"), "size");
    push_string_qualifier(&mut parts, extra.get("followers"), "followers");
    push_string_qualifier(&mut parts, extra.get("created"), "created");
    push_string_qualifier(&mut parts, extra.get("pushed"), "pushed");
    push_string_qualifier(&mut parts, extra.get("updated"), "updated");
    push_string_qualifier(&mut parts, extra.get("merged"), "merged");
    push_string_qualifier(&mut parts, extra.get("closed"), "closed");
    push_string_qualifier(&mut parts, extra.get("good_first_issues"), "good-first-issues");
    push_string_qualifier(&mut parts, extra.get("help_wanted_issues"), "help-wanted-issues");
    push_string_qualifier(&mut parts, extra.get("filename"), "filename");
    push_string_qualifier(&mut parts, extra.get("extension"), "extension");
    push_string_qualifier(&mut parts, extra.get("path"), "path");
    push_string_qualifier(&mut parts, extra.get("state"), "state");
    push_string_qualifier(&mut parts, extra.get("milestone"), "milestone");
    push_string_qualifier(&mut parts, extra.get("linked"), "linked");
    push_string_qualifier(&mut parts, extra.get("type"), "type");
    push_string_qualifier(&mut parts, extra.get("review"), "review");
    push_string_qualifier(&mut parts, extra.get("reviewed_by"), "reviewed-by");
    push_string_qualifier(&mut parts, extra.get("review_requested"), "review-requested");
    push_string_qualifier(
        &mut parts,
        extra.get("team_review_requested"),
        "team-review-requested",
    );
    push_string_qualifier(&mut parts, extra.get("author"), "author");
    push_string_qualifier(&mut parts, extra.get("assignee"), "assignee");
    push_string_qualifier(&mut parts, extra.get("mentions"), "mentions");
    push_string_qualifier(&mut parts, extra.get("team"), "team");
    push_string_qualifier(&mut parts, extra.get("commenter"), "commenter");
    push_string_qualifier(&mut parts, extra.get("involves"), "involves");
    push_string_qualifier(&mut parts, extra.get("comments"), "comments");
    push_string_qualifier(&mut parts, extra.get("interactions"), "interactions");
    push_string_qualifier(&mut parts, extra.get("reactions"), "reactions");
    push_string_qualifier(&mut parts, extra.get("head"), "head");
    push_string_qualifier(&mut parts, extra.get("base"), "base");
    push_string_qualifier(&mut parts, extra.get("status"), "status");
    push_string_qualifier(&mut parts, extra.get("language_in_user"), "language");
    push_string_qualifier(&mut parts, extra.get("location"), "location");
    if let Some(b) = extra.get("archived").and_then(|v| v.as_bool()) {
        parts.push(format!("archived:{b}"));
    }
    if let Some(b) = extra.get("is_mirror").and_then(|v| v.as_bool()) {
        parts.push(format!("mirror:{b}"));
    }
    if let Some(b) = extra.get("is_template").and_then(|v| v.as_bool()) {
        parts.push(format!("template:{b}"));
    }
    if let Some(b) = extra.get("is_draft").and_then(|v| v.as_bool()) {
        parts.push(if b { "is:draft".into() } else { "-is:draft".into() });
    }
    if let Some(true) = extra.get("is_public").and_then(|v| v.as_bool()) {
        parts.push("is:public".into());
    }
    if let Some(true) = extra.get("is_private").and_then(|v| v.as_bool()) {
        parts.push("is:private".into());
    }
    if let Some(f) = extra.get("is_fork").and_then(|v| v.as_str()) {
        match f {
            "only" => parts.push("fork:only".into()),
            "true" => parts.push("fork:true".into()),
            "false" => parts.push("fork:false".into()),
            _ => {}
        }
    }
    if let Some(true) = extra.get("no_label").and_then(|v| v.as_bool()) {
        parts.push("no:label".into());
    }
    if let Some(true) = extra.get("no_milestone").and_then(|v| v.as_bool()) {
        parts.push("no:milestone".into());
    }
    if let Some(true) = extra.get("no_assignee").and_then(|v| v.as_bool()) {
        parts.push("no:assignee".into());
    }
    if let Some(b) = extra.get("draft").and_then(|v| v.as_bool()) {
        parts.push(format!("draft:{b}"));
    }
    parts.join(" ")
}

fn push_array_qualifier<F>(parts: &mut Vec<String>, value: Option<&Value>, mapper: F)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(arr) = value.and_then(|v| v.as_array()) {
        for item in arr.iter().filter_map(|v| v.as_str()) {
            if let Some(part) = mapper(item) {
                parts.push(part);
            }
        }
    }
}

fn push_string_qualifier(parts: &mut Vec<String>, value: Option<&Value>, qualifier: &str) {
    let Some(raw) = value.and_then(|v| v.as_str()) else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let val = if trimmed.contains(' ') {
        format!("\"{trimmed}\"")
    } else {
        trimmed.to_string()
    };
    parts.push(format!("{qualifier}:{val}"));
}
