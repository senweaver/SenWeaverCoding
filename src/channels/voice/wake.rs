// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::channels::pipeline::transcription::transcribe_audio;
use crate::config::TranscriptionConfig;
use crate::config::schema::VoiceWakeConfig;

use super::super::traits::{Channel, ChannelMessage, SendMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeState {

    Listening,

    Triggered,

    Capturing,

    Processing,
}

impl std::fmt::Display for WakeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Listening => write!(f, "Listening"),
            Self::Triggered => write!(f, "Triggered"),
            Self::Capturing => write!(f, "Capturing"),
            Self::Processing => write!(f, "Processing"),
        }
    }
}

pub struct VoiceWakeChannel {
    config: VoiceWakeConfig,
    transcription_config: TranscriptionConfig,
}

impl VoiceWakeChannel {

    pub fn new(config: VoiceWakeConfig, transcription_config: TranscriptionConfig) -> Self {
        Self {
            config,
            transcription_config,
        }
    }
}

#[async_trait]
impl Channel for VoiceWakeChannel {
    fn name(&self) -> &str {
        "voice_wake"
    }

    async fn send(&self, _message: &SendMessage) -> Result<()> {

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        let config = self.config.clone();
        let transcription_config = self.transcription_config.clone();

        let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<f32>>(4);

        let energy_threshold = config.energy_threshold;
        let silence_timeout = Duration::from_millis(u64::from(config.silence_timeout_ms));
        let max_capture = Duration::from_secs(u64::from(config.max_capture_secs));
        let sample_rate: u32;
        let channels_count: u16;
        let _audio_stream;

        {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

            let host = cpal::default_host();
            let device = host
                .default_input_device()
                .ok_or_else(|| anyhow::anyhow!("No default audio input device available"))?;

            let supported = device.default_input_config()?;
            sample_rate = supported.sample_rate().0;
            channels_count = supported.channels();

            info!(
                device = ?device.name().unwrap_or_default(),
                sample_rate,
                channels = channels_count,
                "VoiceWake: opening audio input"
            );

            let stream_config: cpal::StreamConfig = supported.into();
            let audio_tx_clone = audio_tx.clone();

            let stream = device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {

                    let _ = audio_tx_clone.try_send(data.to_vec());
                },
                move |err| {
                    warn!("VoiceWake: audio stream error: {err}");
                },
                None,
            )?;

            stream.play()?;

            _audio_stream = stream;
        }

        drop(audio_tx);

        let wake_word = config.wake_word.to_lowercase();
        let mut state = WakeState::Listening;
        let mut capture_buf: Vec<f32> = Vec::new();
        let mut last_voice_at = Instant::now();
        let mut capture_start = Instant::now();
        let mut msg_counter: u64 = 0;

        info!(wake_word = %wake_word, "VoiceWake: entering listen loop");

        while let Some(chunk) = audio_rx.recv().await {
            let energy = compute_rms_energy(&chunk);

            match state {
                WakeState::Listening => {
                    if energy >= energy_threshold {
                        debug!(
                            energy,
                            "VoiceWake: energy spike  -  transitioning to Triggered"
                        );
                        state = WakeState::Triggered;
                        capture_buf.clear();
                        capture_buf.extend_from_slice(&chunk);
                        last_voice_at = Instant::now();
                        capture_start = Instant::now();
                    }
                }
                WakeState::Triggered => {
                    capture_buf.extend_from_slice(&chunk);

                    if energy >= energy_threshold {
                        last_voice_at = Instant::now();
                    }

                    let since_voice = last_voice_at.elapsed();
                    let since_start = capture_start.elapsed();

                    if since_voice >= silence_timeout || since_start >= max_capture {
                        debug!("VoiceWake: Triggered window closed  -  transcribing for wake word");

                        let wav_bytes =
                            encode_wav_from_f32(&capture_buf, sample_rate, channels_count);

                        match transcribe_audio(wav_bytes, "wake_check.wav", &transcription_config)
                            .await
                        {
                            Ok(text) => {
                                let lower = text.to_lowercase();
                                if lower.contains(&wake_word) {
                                    info!(text = %text, "VoiceWake: wake word detected  -  capturing utterance");
                                    state = WakeState::Capturing;
                                    capture_buf.clear();
                                    last_voice_at = Instant::now();
                                    capture_start = Instant::now();
                                } else {
                                    debug!(text = %text, "VoiceWake: no wake word  -  back to Listening");
                                    state = WakeState::Listening;
                                    capture_buf.clear();
                                }
                            }
                            Err(e) => {
                                warn!("VoiceWake: transcription error during wake check: {e}");
                                state = WakeState::Listening;
                                capture_buf.clear();
                            }
                        }
                    }
                }
                WakeState::Capturing => {
                    capture_buf.extend_from_slice(&chunk);

                    if energy >= energy_threshold {
                        last_voice_at = Instant::now();
                    }

                    let since_voice = last_voice_at.elapsed();
                    let since_start = capture_start.elapsed();

                    if since_voice >= silence_timeout || since_start >= max_capture {
                        debug!("VoiceWake: utterance capture complete  -  transcribing");

                        let wav_bytes =
                            encode_wav_from_f32(&capture_buf, sample_rate, channels_count);

                        match transcribe_audio(wav_bytes, "utterance.wav", &transcription_config)
                            .await
                        {
                            Ok(text) => {
                                let trimmed = text.trim().to_string();
                                if !trimmed.is_empty() {
                                    msg_counter += 1;
                                    let ts = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs();

                                    let msg = ChannelMessage {
                                        id: format!("voice_wake_{msg_counter}"),
                                        sender: "voice_user".into(),
                                        reply_target: "voice_user".into(),
                                        content: trimmed,
                                        channel: "voice_wake".into(),
                                        timestamp: ts,
                                        thread_ts: None,
                                        interruption_scope_id: None,
                                        attachments: vec![],
                                    };

                                    if let Err(e) = tx.send(msg).await {
                                        warn!("VoiceWake: failed to dispatch message: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("VoiceWake: transcription error for utterance: {e}");
                            }
                        }

                        state = WakeState::Listening;
                        capture_buf.clear();
                    }
                }
                WakeState::Processing => {

                }
            }
        }

        bail!("VoiceWake: audio stream ended unexpectedly");
    }
}

pub fn compute_rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

pub fn encode_wav_from_f32(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate = u32::from(channels) * sample_rate * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    #[allow(clippy::cast_possible_truncation)]
    let data_len = (samples.len() * 2) as u32;
    let file_len = 36 + data_len;

    let mut buf = Vec::with_capacity(file_len as usize + 8);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_len.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        #[allow(clippy::cast_possible_truncation)]
        let pcm16 = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&pcm16.to_le_bytes());
    }

    buf
}
