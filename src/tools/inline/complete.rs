// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::inline_completion::context_builder::build_context_from_window;
use crate::inline_completion::{
    InlineCompletionError, InlineCompletionRequest, Language, RegistryHandle,
};
use crate::tools::traits::{Tool, ToolResult};

#[derive(Debug)]
pub struct InlineCompleteTool {
    registry: Option<RegistryHandle>,
}

impl Default for InlineCompleteTool {
    fn default() -> Self {
        Self { registry: None }
    }
}

impl InlineCompleteTool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_registry(registry: RegistryHandle) -> Self {
        Self {
            registry: Some(registry),
        }
    }

    fn registry(&self) -> Option<RegistryHandle> {
        self.registry.clone()
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    prefix: String,
    #[serde(default)]
    suffix: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
    #[serde(default)]
    stop_sequences: Vec<String>,
    #[serde(default = "default_top_k")]
    top_k: u32,
}

fn default_max_tokens() -> u32 {
    128
}

fn default_top_k() -> u32 {
    1
}

fn parse_language(label: Option<&str>) -> Language {
    let Some(label) = label else {
        return Language::Other;
    };
    match label.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => Language::Rust,
        "typescript" | "ts" | "tsx" => Language::TypeScript,
        "javascript" | "js" | "jsx" => Language::JavaScript,
        "python" | "py" => Language::Python,
        "go" | "golang" => Language::Go,
        "java" => Language::Java,
        "c++" | "cpp" | "cxx" | "cc" => Language::Cpp,
        "c" => Language::C,
        "csharp" | "cs" | "c#" => Language::CSharp,
        "ruby" | "rb" => Language::Ruby,
        "php" => Language::Php,
        "swift" => Language::Swift,
        "kotlin" | "kt" => Language::Kotlin,
        "scala" => Language::Scala,
        "shell" | "sh" | "bash" | "zsh" | "powershell" | "ps1" => Language::Shell,
        "html" => Language::Html,
        "css" => Language::Css,
        "json" => Language::Json,
        "yaml" | "yml" => Language::Yaml,
        "toml" => Language::Toml,
        "markdown" | "md" => Language::Markdown,
        "sql" => Language::Sql,
        _ => Language::Other,
    }
}

#[async_trait]
impl Tool for InlineCompleteTool {
    fn name(&self) -> &str {
        "inline_complete"
    }

    fn description(&self) -> &str {
        "Generate inline / fill-in-the-middle code completions for the cursor position. \
         Returns one or more candidate insertion strings.  Pure read-only  -  never modifies \
         workspace files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prefix"],
            "properties": {
                "prefix": {
                    "type": "string",
                    "description": "Text before the cursor (required)."
                },
                "suffix": {
                    "type": "string",
                    "description": "Text after the cursor (optional, used by FIM models).",
                    "default": ""
                },
                "language": {
                    "type": "string",
                    "description": "Source language hint (rust / typescript / python / …).",
                    "default": null
                },
                "file_path": {
                    "type": "string",
                    "description": "Workspace-relative file path used for cache locality.",
                    "default": null
                },
                "max_tokens": {
                    "type": "integer",
                    "description": "Maximum tokens for the suggestion.",
                    "default": 128,
                    "minimum": 1
                },
                "stop_sequences": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional explicit stop sequences."
                },
                "top_k": {
                    "type": "integer",
                    "description": "Number of candidates to keep (capped at 5).",
                    "default": 1,
                    "minimum": 1,
                    "maximum": 5
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let parsed: Args = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("invalid arguments: {e}")),
                });
            }
        };

        let Some(registry) = self.registry() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "inline completion is disabled  -  no LLM provider is configured. \
                     Run `sen config wizard` and try again."
                        .to_string(),
                ),
            });
        };

        let language = parse_language(parsed.language.as_deref());
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let file_path = parsed
            .file_path
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| workspace_root.join("<scratch>"));
        let context = build_context_from_window(&parsed.prefix, &parsed.suffix);
        let req = InlineCompletionRequest {
            prefix: parsed.prefix,
            suffix: parsed.suffix,
            language,
            file_path,
            workspace_root,
            context,
            max_tokens: parsed.max_tokens,
            stop_sequences: parsed.stop_sequences,
            request_id: uuid::Uuid::new_v4(),
        };

        match registry.request(req).await {
            Ok(resp) => {
                let suggestions: Vec<Value> = resp
                    .suggestions
                    .iter()
                    .take(parsed.top_k.max(1) as usize)
                    .map(|s| {
                        json!({
                            "insert_text": s.insert_text,
                            "rationale": s.rationale,
                            "confidence": s.confidence,
                        })
                    })
                    .collect();
                let payload = json!({
                    "provider": resp.provider,
                    "latency_ms": resp.latency_ms,
                    "cached": resp.cached,
                    "suggestions": suggestions,
                });
                Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
                        format!(
                            "{{\"provider\":\"{}\",\"suggestions\":{}}}",
                            resp.provider,
                            resp.suggestions.len()
                        )
                    }),
                    error: None,
                })
            }
            Err(InlineCompletionError::Empty { provider }) => Ok(ToolResult {
                success: true,
                output: serde_json::to_string(&json!({
                    "provider": provider,
                    "suggestions": Vec::<Value>::new(),
                    "latency_ms": 0,
                    "cached": false,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
                error: None,
            }),
            Err(e) => {
                let err: InlineCompletionError = e;
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(err.to_string()),
                })
            }
        }
    }

    fn fingerprint(&self, args: &Value) -> Option<String> {
        let prefix = args.get("prefix")?.as_str()?;
        let suffix = args.get("suffix").and_then(Value::as_str).unwrap_or("");
        let language = args.get("language").and_then(Value::as_str).unwrap_or("");
        Some(format!(
            "inline_complete::{language}::{}::{}",
            prefix.len(),
            suffix.len()
        ))
    }

    fn cache_ttl_secs(&self) -> u64 {

        15
    }
}
