// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::client::{ImageGenerator, LinkedInClient};
use super::super::traits::{Tool, ToolResult};
use crate::config::{LinkedInContentConfig, LinkedInImageConfig};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct LinkedInTool {
    security: Arc<SecurityPolicy>,
    workspace_dir: PathBuf,
    api_version: String,
    content_config: LinkedInContentConfig,
    image_config: LinkedInImageConfig,
}

impl LinkedInTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        workspace_dir: PathBuf,
        api_version: String,
        content_config: LinkedInContentConfig,
        image_config: LinkedInImageConfig,
    ) -> Self {
        Self {
            security,
            workspace_dir,
            api_version,
            content_config,
            image_config,
        }
    }

    fn is_write_action(action: &str) -> bool {
        matches!(action, "create_post" | "comment" | "react" | "delete_post")
    }

    fn build_content_strategy_summary(&self) -> String {
        let c = &self.content_config;
        let mut parts = Vec::new();

        if !c.persona.is_empty() {
            parts.push(format!("## Persona\n{}", c.persona));
        }

        if !c.topics.is_empty() {
            parts.push(format!("## Topics\n{}", c.topics.join(", ")));
        }

        if !c.rss_feeds.is_empty() {
            let feeds: Vec<String> = c.rss_feeds.iter().map(|f| format!("- {f}")).collect();
            parts.push(format!(
                "## RSS Feeds (fetch titles only for inspiration)\n{}",
                feeds.join("\n")
            ));
        }

        if !c.github_users.is_empty() {
            parts.push(format!(
                "## GitHub Users (check public activity)\n{}",
                c.github_users.join(", ")
            ));
        }

        if !c.github_repos.is_empty() {
            let repos: Vec<String> = c.github_repos.iter().map(|r| format!("- {r}")).collect();
            parts.push(format!(
                "## GitHub Repos (highlight project work)\n{}",
                repos.join("\n")
            ));
        }

        if !c.instructions.is_empty() {
            parts.push(format!("## Posting Instructions\n{}", c.instructions));
        }

        if parts.is_empty() {
            return "No content strategy configured. Add [linkedin.content] settings to config.toml with rss_feeds, github_repos, persona, topics, and instructions.".to_string();
        }

        parts.join("\n\n")
    }
}

#[async_trait]
impl Tool for LinkedInTool {
    fn name(&self) -> &str {
        "linkedin"
    }

    fn description(&self) -> &str {
        "Manage LinkedIn: create posts, list your posts, comment, react, delete posts, view engagement, get profile info, and read the configured content strategy. Requires LINKEDIN_* credentials in .env file."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "create_post",
                        "list_posts",
                        "comment",
                        "react",
                        "delete_post",
                        "get_engagement",
                        "get_profile",
                        "get_content_strategy"
                    ],
                    "description": "The LinkedIn action to perform"
                },
                "text": {
                    "type": "string",
                    "description": "Post or comment text content"
                },
                "visibility": {
                    "type": "string",
                    "enum": ["PUBLIC", "CONNECTIONS"],
                    "description": "Post visibility (default: PUBLIC)"
                },
                "article_url": {
                    "type": "string",
                    "description": "URL for link preview in a post"
                },
                "article_title": {
                    "type": "string",
                    "description": "Title for the article (requires article_url)"
                },
                "post_id": {
                    "type": "string",
                    "description": "LinkedIn post URN identifier"
                },
                "reaction_type": {
                    "type": "string",
                    "enum": ["LIKE", "CELEBRATE", "SUPPORT", "LOVE", "INSIGHTFUL", "FUNNY"],
                    "description": "Type of reaction to add to a post"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of posts to retrieve (default 10, max 50)"
                },
                "generate_image": {
                    "type": "boolean",
                    "description": "Generate an AI image for the post (requires [linkedin.image] config). Falls back to branded SVG card if all providers fail."
                },
                "image_prompt": {
                    "type": "string",
                    "description": "Custom prompt for image generation. If omitted, a prompt is derived from the post text."
                },
                "scheduled_at": {
                    "type": "string",
                    "description": "Schedule the post for future publication. ISO 8601 / RFC 3339 timestamp, e.g. '2026-03-17T08:00:00Z'. The post is saved as a draft with scheduledPublishTime on LinkedIn."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'action' parameter"))?;

        if Self::is_write_action(action) && !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: rate limit exceeded".into()),
            });
        }

        let client = LinkedInClient::new(self.workspace_dir.clone(), self.api_version.clone());

        match action {
            "get_content_strategy" => {
                let strategy = self.build_content_strategy_summary();
                return Ok(ToolResult {
                    success: true,
                    output: strategy,
                    error: None,
                });
            }
            "create_post" => {
                let text = match args.get("text").and_then(|v| v.as_str()).map(str::trim) {
                    Some(t) if !t.is_empty() => t.to_string(),
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("Missing required 'text' parameter for create_post".into()),
                        });
                    }
                };

                let visibility = args
                    .get("visibility")
                    .and_then(|v| v.as_str())
                    .unwrap_or("PUBLIC");

                let generate_image = args
                    .get("generate_image")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let article_url = args.get("article_url").and_then(|v| v.as_str());
                let article_title = args.get("article_title").and_then(|v| v.as_str());
                let scheduled_at = args.get("scheduled_at").and_then(|v| v.as_str());

                if article_title.is_some() && article_url.is_none() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'article_title' requires 'article_url' to be provided".into()),
                    });
                }

                if generate_image && self.image_config.enabled {
                    let image_prompt = args
                        .get("image_prompt")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| {
                            format!(
                                "Professional, modern illustration for a LinkedIn post about: {}",
                                crate::util::truncate_str_bytes(&text, 200)
                            )
                        });

                    let generator =
                        ImageGenerator::new(self.image_config.clone(), self.workspace_dir.clone());

                    match generator.generate(&image_prompt).await {
                        Ok(image_path) => {
                            let image_bytes = tokio::fs::read(&image_path).await?;
                            let creds = client.get_credentials().await?;
                            let image_urn = client
                                .upload_image(&image_bytes, &creds.access_token, &creds.person_id)
                                .await?;

                            let post_id = client
                                .create_post_with_image(&text, visibility, &image_urn, scheduled_at)
                                .await?;

                            let _ = ImageGenerator::cleanup(&image_path).await;

                            let action_word = if scheduled_at.is_some() {
                                "scheduled"
                            } else {
                                "published"
                            };
                            return Ok(ToolResult {
                                success: true,
                                output: format!(
                                    "Post {action_word} with image. Post ID: {post_id}, Image: {image_urn}"
                                ),
                                error: None,
                            });
                        }
                        Err(e) => {

                            tracing::warn!("Image generation failed, posting without image: {e}");
                        }
                    }
                }

                let post_id = client
                    .create_post(&text, visibility, article_url, article_title, scheduled_at)
                    .await?;

                let action_word = if scheduled_at.is_some() {
                    "scheduled"
                } else {
                    "published"
                };
                Ok(ToolResult {
                    success: true,
                    output: format!("Post {action_word} successfully. Post ID: {post_id}"),
                    error: None,
                })
            }

            "list_posts" => {
                let count = args
                    .get("count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10)
                    .clamp(1, 50) as usize;

                let posts = client.list_posts(count).await?;

                Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string(&posts)?,
                    error: None,
                })
            }

            "comment" => {
                let post_id = match args.get("post_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("Missing required 'post_id' parameter for comment".into()),
                        });
                    }
                };

                let text = match args.get("text").and_then(|v| v.as_str()).map(str::trim) {
                    Some(t) if !t.is_empty() => t.to_string(),
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("Missing required 'text' parameter for comment".into()),
                        });
                    }
                };

                let comment_id = client.add_comment(post_id, &text).await?;

                Ok(ToolResult {
                    success: true,
                    output: format!("Comment posted successfully. Comment ID: {comment_id}"),
                    error: None,
                })
            }

            "react" => {
                let post_id = match args.get("post_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("Missing required 'post_id' parameter for react".into()),
                        });
                    }
                };

                let reaction_type = match args.get("reaction_type").and_then(|v| v.as_str()) {
                    Some(rt) if !rt.is_empty() => rt,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(
                                "Missing required 'reaction_type' parameter for react".into(),
                            ),
                        });
                    }
                };

                client.add_reaction(post_id, reaction_type).await?;

                Ok(ToolResult {
                    success: true,
                    output: format!("Reaction '{reaction_type}' added to post {post_id}"),
                    error: None,
                })
            }

            "delete_post" => {
                let post_id = match args.get("post_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(
                                "Missing required 'post_id' parameter for delete_post".into(),
                            ),
                        });
                    }
                };

                client.delete_post(post_id).await?;

                Ok(ToolResult {
                    success: true,
                    output: format!("Post {post_id} deleted successfully"),
                    error: None,
                })
            }

            "get_engagement" => {
                let post_id = match args.get("post_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(
                                "Missing required 'post_id' parameter for get_engagement".into(),
                            ),
                        });
                    }
                };

                let engagement = client.get_engagement(post_id).await?;

                Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string(&engagement)?,
                    error: None,
                })
            }

            "get_profile" => {
                let profile = client.get_profile().await?;

                Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string(&profile)?,
                    error: None,
                })
            }

            unknown => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown action: '{unknown}'")),
            }),
        }
    }
}
