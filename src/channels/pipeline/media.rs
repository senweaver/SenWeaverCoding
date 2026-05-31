// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::{MediaPipelineConfig, TranscriptionConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Image,
    Video,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct MediaAttachment {

    pub file_name: String,

    pub data: Vec<u8>,

    pub mime_type: Option<String>,
}

impl MediaAttachment {

    pub fn kind(&self) -> MediaKind {

        if let Some(ref mime) = self.mime_type {
            let lower = mime.to_ascii_lowercase();
            if lower.starts_with("audio/") {
                return MediaKind::Audio;
            }
            if lower.starts_with("image/") {
                return MediaKind::Image;
            }
            if lower.starts_with("video/") {
                return MediaKind::Video;
            }
        }

        let ext = self
            .file_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "flac" | "mp3" | "mpeg" | "mpga" | "m4a" | "ogg" | "oga" | "opus" | "wav" | "webm" => {
                MediaKind::Audio
            }
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "heic" | "tiff" | "svg" => {
                MediaKind::Image
            }
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" => MediaKind::Video,
            _ => MediaKind::Unknown,
        }
    }
}

pub struct MediaPipeline<'a> {
    config: &'a MediaPipelineConfig,
    transcription_config: &'a TranscriptionConfig,
    vision_available: bool,
}

impl<'a> MediaPipeline<'a> {

    pub fn new(
        config: &'a MediaPipelineConfig,
        transcription_config: &'a TranscriptionConfig,
        vision_available: bool,
    ) -> Self {
        Self {
            config,
            transcription_config,
            vision_available,
        }
    }

    pub async fn process(&self, original_text: &str, attachments: &[MediaAttachment]) -> String {
        if !self.config.enabled || attachments.is_empty() {
            return original_text.to_string();
        }

        let mut annotations = Vec::new();

        for attachment in attachments {
            match attachment.kind() {
                MediaKind::Audio if self.config.transcribe_audio => {
                    let annotation = self.process_audio(attachment).await;
                    annotations.push(annotation);
                }
                MediaKind::Image if self.config.describe_images => {
                    let annotation = self.process_image(attachment);
                    annotations.push(annotation);
                }
                MediaKind::Video if self.config.summarize_video => {
                    let annotation = self.process_video(attachment);
                    annotations.push(annotation);
                }
                _ => {}
            }
        }

        if annotations.is_empty() {
            return original_text.to_string();
        }

        let mut enriched = String::with_capacity(
            annotations.iter().map(|a| a.len() + 1).sum::<usize>() + original_text.len() + 2,
        );

        for annotation in &annotations {
            enriched.push_str(annotation);
            enriched.push('\n');
        }

        if !original_text.is_empty() {
            enriched.push('\n');
            enriched.push_str(original_text);
        }

        enriched.trim().to_string()
    }

    async fn process_audio(&self, attachment: &MediaAttachment) -> String {
        if !self.transcription_config.enabled {
            return "[Audio: attached]".to_string();
        }

        match super::transcription::transcribe_audio(
            attachment.data.clone(),
            &attachment.file_name,
            self.transcription_config,
        )
        .await
        {
            Ok(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    "[Audio transcription: (empty)]".to_string()
                } else {
                    format!("[Audio transcription: {trimmed}]")
                }
            }
            Err(err) => {
                tracing::warn!(
                    file = %attachment.file_name,
                    error = %err,
                    "Media pipeline: audio transcription failed"
                );
                "[Audio: transcription failed]".to_string()
            }
        }
    }

    fn process_image(&self, attachment: &MediaAttachment) -> String {
        if self.vision_available {
            format!(
                "[Image: {} attached, will be processed by vision model]",
                attachment.file_name
            )
        } else {
            format!("[Image: {} attached]", attachment.file_name)
        }
    }

    fn process_video(&self, attachment: &MediaAttachment) -> String {
        format!("[Video: {} attached]", attachment.file_name)
    }
}
