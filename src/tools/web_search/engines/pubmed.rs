// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::truncate_chars;
use async_trait::async_trait;

pub struct PubMedEngine;

#[async_trait]
impl SearchEngine for PubMedEngine {
    fn id(&self) -> &'static str {
        "pubmed"
    }

    fn label(&self) -> &'static str {
        "PubMed"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.max(1);
        let email = ctx
            .api_keys
            .pubmed_email
            .clone()
            .unwrap_or_else(|| "senweavercoding@example.com".to_string());
        let tool_param = "tool=SenWeaverCoding";
        let email_param = format!("email={}", urlencoding::encode(&email));

        let search_url = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term={encoded}&retmax={limit}&retmode=json&sort=relevance&{tool_param}&{email_param}"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&search_url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("PubMed esearch failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let pmids: Vec<String> = json
            .get("esearchresult")
            .and_then(|r| r.get("idlist"))
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if pmids.is_empty() {
            return Ok(Vec::new());
        }
        let ids = pmids.join(",");
        let summary_url = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id={ids}&retmode=json&{tool_param}&{email_param}"
        );
        let summary_resp = client
            .get(&summary_url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !summary_resp.status().is_success() {
            anyhow::bail!("PubMed esummary failed with status: {}", summary_resp.status());
        }
        let summary_json: serde_json::Value = summary_resp.json().await?;
        let result = summary_json.get("result").cloned().unwrap_or_default();
        let mut hits = Vec::new();
        for pmid in pmids.iter() {
            if hits.len() >= ctx.limit {
                break;
            }
            let Some(article) = result.get(pmid) else {
                continue;
            };
            let title = article
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            if title.is_empty() {
                continue;
            }
            let url = format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/");
            let authors = article
                .get("authors")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
                        .take(4)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let journal = article
                .get("fulljournalname")
                .and_then(|j| j.as_str())
                .or_else(|| article.get("source").and_then(|s| s.as_str()))
                .unwrap_or("")
                .to_string();
            let pubdate = article
                .get("pubdate")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let mut desc = String::new();
            if !authors.is_empty() {
                desc.push_str(&format!("[{authors}] "));
            }
            if !journal.is_empty() {
                desc.push_str(&format!("{journal}. "));
            }
            if !pubdate.is_empty() {
                desc.push_str(&format!("({pubdate})"));
            }
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(truncate_chars(&desc, 320))
                .with_source("PubMed");
            if !pubdate.is_empty() {
                hit = hit.with_published_at(pubdate);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
