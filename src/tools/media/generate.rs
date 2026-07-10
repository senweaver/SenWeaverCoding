// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::adapters::{audio, image, video, MediaJob};
use super::credentials;
use super::hyperframes::{self, HyperframesOutput};
use super::registry::{self, MediaSurface};
use crate::security::policy::ToolOperation;
use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct MediaGenTool {
    security: Arc<SecurityPolicy>,
    workspace_dir: PathBuf,
}

impl MediaGenTool {
    pub fn new(security: Arc<SecurityPolicy>, workspace_dir: PathBuf) -> Self {
        Self {
            security,
            workspace_dir,
        }
    }

    fn http_client(services: &crate::services::ServiceContainer) -> reqwest::Client {
        services
            .proxy_runtime()
            .build_client_with_timeouts("tool.media_generate", 600, 15)
    }

    fn safe_stem(name: &str, fallback: &str) -> String {
        let stem = PathBuf::from(name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| fallback.to_string());
        stem.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect()
    }

    fn u64_arg(args: &serde_json::Value, keys: &[&str]) -> Option<u64> {
        for key in keys {
            if let Some(v) = args.get(*key) {
                if let Some(n) = v.as_u64() {
                    return Some(n);
                }
                if let Some(s) = v.as_str() {
                    if let Ok(n) = s.trim().trim_end_matches('s').parse::<u64>() {
                        return Some(n);
                    }
                }
            }
        }
        None
    }

    fn str_arg<'a>(args: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
        for key in keys {
            if let Some(s) = args.get(*key).and_then(|v| v.as_str()) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        None
    }

    async fn resolve_workspace_file(&self, raw: &str, what: &str) -> anyhow::Result<PathBuf> {
        let cleaned = raw.replace('\\', "/");
        let p = PathBuf::from(&cleaned);
        let abs = if p.is_absolute() {
            p
        } else {
            self.workspace_dir.join(p)
        };
        let canon = tokio::fs::canonicalize(&abs)
            .await
            .map_err(|_| anyhow::anyhow!("{what} `{raw}` not found in the workspace"))?;
        let ws = tokio::fs::canonicalize(&self.workspace_dir)
            .await
            .unwrap_or_else(|_| self.workspace_dir.clone());
        if !canon.starts_with(&ws) {
            anyhow::bail!("{what} `{raw}` resolves outside the workspace");
        }
        if !canon.is_file() {
            anyhow::bail!("{what} `{raw}` is not a file");
        }
        Ok(canon)
    }

    fn unique_output_path(dir: &std::path::Path, stem: &str, ext: &str) -> PathBuf {
        let mut path = dir.join(format!("{stem}.{ext}"));
        let mut k = 2;
        while path.exists() {
            path = dir.join(format!("{stem}-{k}.{ext}"));
            k += 1;
        }
        path
    }

    fn infer_provider(surface: MediaSurface, model: &str) -> Option<String> {
        if registry::is_fal_model(model) {
            return Some("fal".to_string());
        }
        if surface == MediaSurface::Image && registry::is_gemini_image_model(model) {
            return Some("gemini".to_string());
        }
        registry::default_models(surface)
            .as_array()
            .and_then(|models| {
                models.iter().find(|m| {
                    m.get("id")
                        .and_then(|v| v.as_str())
                        .map(|id| id.eq_ignore_ascii_case(model))
                        .unwrap_or(false)
                })
            })
            .and_then(|m| m.get("provider").and_then(|v| v.as_str()))
            .filter(|p| !p.is_empty() && *p != "hyperframes")
            .map(str::to_string)
    }

    fn output_dir(&self, subdir: &str) -> PathBuf {
        if matches!(
            crate::agent::coding_mode::active_coding_mode(),
            crate::agent::coding_mode::CodingMode::Designer
        ) {
            if let Some(session) = crate::session::current_session_context() {
                let rel =
                    crate::agent::designer::pipeline::designer_session_dir(&session.session_id);
                return self.workspace_dir.join(rel).join(subdir);
            }
        }
        self.workspace_dir.join(subdir)
    }

    async fn run(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let surface_raw = args
            .get("surface")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let Some(surface) = MediaSurface::from_str(surface_raw) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing/invalid 'surface' (expected image|video|audio)".into()),
            });
        };

        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let composition_dir = args
            .get("composition_dir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());

        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"))
            .map(str::to_string)
            .unwrap_or_else(|| {
                registry::default_models(surface)
                    .get(0)
                    .and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("gpt-image-1")
                    .to_string()
            });

        let provider = args.get("provider").and_then(|v| v.as_str());
        let aspect = args
            .get("aspect")
            .and_then(|v| v.as_str())
            .unwrap_or("1:1")
            .to_string();
        let audio_kind = Self::str_arg(&args, &["audio_kind", "audioKind"])
            .unwrap_or("speech")
            .to_string();
        let voice = args
            .get("voice")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty());
        let count = Self::u64_arg(&args, &["count"]).unwrap_or(1).clamp(1, 4) as usize;
        let resolution = Self::str_arg(&args, &["resolution"])
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| matches!(s.as_str(), "2k" | "4k"));
        let seconds = Self::u64_arg(&args, &["length", "duration"]).unwrap_or(match surface {
            MediaSurface::Video => 5,
            MediaSurface::Audio => 15,
            MediaSurface::Image => 0,
        }) as u32;
        let filename = args
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("designer");

        let mut source_image_path: Option<PathBuf> = None;
        let mut mask_path: Option<PathBuf> = None;
        let mut fidelity: Option<String> = None;
        if surface == MediaSurface::Image {
            if let Some(raw) = Self::str_arg(&args, &["source_image", "sourceImage", "image"]) {
                match self.resolve_workspace_file(raw, "source_image").await {
                    Ok(p) => source_image_path = Some(p),
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
            if let Some(raw) = Self::str_arg(&args, &["mask", "mask_image", "maskImage"]) {
                if source_image_path.is_none() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(
                            "'mask' requires 'source_image' — a mask only makes sense when editing an existing image".into(),
                        ),
                    });
                }
                match self.resolve_workspace_file(raw, "mask").await {
                    Ok(p) => mask_path = Some(p),
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
            fidelity = Self::str_arg(&args, &["fidelity", "input_fidelity", "inputFidelity"])
                .map(str::to_ascii_lowercase)
                .filter(|f| matches!(f.as_str(), "high" | "low"));
        }

        if registry::is_hyperframes_model(&model) {
            return self
                .run_hyperframes(composition_dir, filename, seconds.max(2))
                .await;
        }

        if prompt.is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing required parameter: 'prompt'".into()),
            });
        }
        let prompt = prompt.unwrap().to_string();

        let Some(services) = crate::services::try_get_services() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "media generation blocked: service container unavailable (fail-closed)"
                        .into(),
                ),
            });
        };
        let config = services.config();
        let inferred_provider = if provider.is_none() {
            Self::infer_provider(surface, &model)
        } else {
            None
        };
        let resolved = credentials::resolve(
            &config,
            provider.or(inferred_provider.as_deref()),
            &model,
        );
        let client = Self::http_client(services);

        let make_job = |p: String| MediaJob {
            client: client.clone(),
            provider: resolved.clone(),
            model: model.clone(),
            prompt: p,
            aspect: aspect.clone(),
            seconds,
            voice: voice.clone(),
            audio_kind: audio_kind.clone(),
            resolution: resolution.clone(),
            source_image: source_image_path.clone(),
            mask: mask_path.clone(),
            fidelity: fidelity.clone(),
        };

        let (subdir, ext) = match surface {
            MediaSurface::Image => ("images", registry::MediaSurface::Image.extension(None)),
            MediaSurface::Video => ("videos", registry::MediaSurface::Video.extension(None)),
            MediaSurface::Audio => (
                "audio",
                registry::MediaSurface::Audio.extension(Some(&audio_kind)),
            ),
        };
        let out_dir = self.output_dir(subdir);
        tokio::fs::create_dir_all(&out_dir).await.ok();
        let stem = if source_image_path.is_some() && filename == "designer" {
            let src_stem = source_image_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| Self::safe_stem(&s.to_string_lossy(), "designer"))
                .unwrap_or_else(|| "designer".to_string());
            format!("{src_stem}-edit")
        } else {
            Self::safe_stem(filename, "designer")
        };

        let mut saved: Vec<String> = Vec::new();
        let iterations = if surface == MediaSurface::Image { count } else { 1 };
        for i in 0..iterations {
            let job = make_job(prompt.clone());
            let bytes = match surface {
                MediaSurface::Image => image::generate(&job).await,
                MediaSurface::Video => video::generate(&job).await,
                MediaSurface::Audio => audio::generate(&job).await,
            };
            let bytes = match bytes {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "{surface_raw} generation failed (provider '{}', model '{model}'): {e}",
                            resolved.provider_id
                        )),
                    });
                }
            };
            let name_base = if iterations > 1 {
                format!("{stem}_{}", i + 1)
            } else {
                stem.clone()
            };
            let path = Self::unique_output_path(&out_dir, &name_base, ext);
            tokio::fs::write(&path, &bytes).await?;
            crate::agent::designer::record_artifact_if_designer(&path);
            saved.push(path.display().to_string());
        }

        let mode_label = if source_image_path.is_some() {
            if mask_path.is_some() {
                "edited (masked region repaint)"
            } else {
                "edited (whole-image)"
            }
        } else {
            "generated"
        };
        Ok(ToolResult {
            success: true,
            output: format!(
                "{} {} {} via provider '{}' (model '{model}').\nFiles:\n{}",
                saved.len(),
                surface_raw,
                mode_label,
                resolved.provider_id,
                saved.join("\n")
            ),
            error: None,
        })
    }

    async fn run_hyperframes(
        &self,
        composition_dir: Option<&str>,
        filename: &str,
        seconds: u32,
    ) -> anyhow::Result<ToolResult> {
        let Some(dir) = composition_dir else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "HyperFrames requires 'composition_dir' pointing at a folder containing index.html"
                        .into(),
                ),
            });
        };
        let comp_path = {
            let p = PathBuf::from(dir);
            if p.is_absolute() {
                p
            } else {
                self.workspace_dir.join(p)
            }
        };
        let out_dir = self.output_dir("videos");
        tokio::fs::create_dir_all(&out_dir).await.ok();
        let stem = Self::safe_stem(filename, "hyperframes");
        let out_path = Self::unique_output_path(&out_dir, &stem, "mp4");

        match hyperframes::render(&comp_path, &out_path, seconds).await {
            Ok(HyperframesOutput::Mp4(path)) => {
                crate::agent::designer::record_artifact_if_designer(&path);
                Ok(ToolResult {
                    success: true,
                    output: format!("HyperFrames rendered to MP4.\nFile: {}", path.display()),
                    error: None,
                })
            }
            Ok(HyperframesOutput::LiveHtml(path)) => {
                crate::agent::designer::record_artifact_if_designer(&path);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "HyperFrames MP4 renderer unavailable; the composition plays live in the preview \
                         panel.\nComposition: {}",
                        path.display()
                    ),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("HyperFrames render failed: {e}")),
            }),
        }
    }
}

#[async_trait]
impl Tool for MediaGenTool {
    fn name(&self) -> &str {
        "media_generate"
    }

    fn description(&self) -> &str {
        "Generate or EDIT real media files (image / video / audio) for Designer mode, reusing the \
         user's configured model providers for credentials and base URLs. Image editing: pass \
         'source_image' (workspace path) for whole-image instruction edits, plus 'mask' for \
         masked-region repaint (white = repaint); edited results are written as NEW files. Saves \
         outputs into the workspace (images/ videos/ audio/) and returns the file paths. Also \
         renders HyperFrames HTML compositions to MP4 via 'model'='hyperframes-html' + \
         'composition_dir'."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["surface"],
            "properties": {
                "surface": {
                    "type": "string",
                    "enum": ["image", "video", "audio"],
                    "description": "Media surface to generate."
                },
                "prompt": {
                    "type": "string",
                    "description": "Generation prompt (required except for hyperframes render)."
                },
                "model": {
                    "type": "string",
                    "description": "Model id from the configured providers (or 'hyperframes-html' for local video render)."
                },
                "provider": {
                    "type": "string",
                    "description": "Optional provider id override; otherwise inferred from the model or the default provider."
                },
                "aspect": {
                    "type": "string",
                    "description": "Aspect ratio (1:1, 16:9, 9:16, 4:3, 3:4)."
                },
                "length": {
                    "type": "integer",
                    "description": "Video length in seconds."
                },
                "duration": {
                    "type": "integer",
                    "description": "Audio/HyperFrames duration in seconds."
                },
                "voice": {
                    "type": "string",
                    "description": "Voice id/name for speech audio."
                },
                "audio_kind": {
                    "type": "string",
                    "enum": ["speech", "sfx", "music"],
                    "description": "Audio kind (default speech)."
                },
                "count": {
                    "type": "integer",
                    "description": "Number of images to generate (1-4, image surface only)."
                },
                "source_image": {
                    "type": "string",
                    "description": "Image surface only: workspace-relative path of an EXISTING image to edit. With 'mask' → masked region repaint (inpaint); without → whole-image instruction edit / variation. The edited result is written as a NEW file, never overwriting the source."
                },
                "mask": {
                    "type": "string",
                    "description": "Image surface only: workspace-relative path of a grayscale mask PNG (WHITE = region to repaint, BLACK = keep). Requires 'source_image'. Polarity conversion per provider is handled automatically."
                },
                "fidelity": {
                    "type": "string",
                    "enum": ["high", "low"],
                    "description": "Image edit only: how closely the untouched parts must match the source (high = preserve faithfully, default)."
                },
                "resolution": {
                    "type": "string",
                    "enum": ["2k", "4k"],
                    "description": "Optional high-resolution request for image surface (provider permitting)."
                },
                "composition_dir": {
                    "type": "string",
                    "description": "HyperFrames composition folder (must contain index.html)."
                },
                "filename": {
                    "type": "string",
                    "description": "Output filename stem (default 'designer')."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "media_generate")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }
        self.run(args).await
    }
}
