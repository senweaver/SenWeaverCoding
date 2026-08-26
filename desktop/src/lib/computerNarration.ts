// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { uploadAudioSegment } from '../api/computer'

export type MicrophoneDevice = {
  id: string
  label: string
}

export const NARRATION_LANGUAGES: { code: string; label: string }[] = [
  { code: 'en', label: 'English' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日本語' },
  { code: 'ko', label: '한국어' },
  { code: 'es', label: 'Español' },
  { code: 'fr', label: 'Français' },
  { code: 'de', label: 'Deutsch' },
  { code: 'it', label: 'Italiano' },
  { code: 'pt', label: 'Português' },
  { code: 'ru', label: 'Русский' },
  { code: 'ar', label: 'العربية' },
  { code: 'hi', label: 'हिन्दी' },
  { code: 'nl', label: 'Nederlands' },
  { code: 'tr', label: 'Türkçe' },
  { code: 'vi', label: 'Tiếng Việt' },
  { code: 'id', label: 'Bahasa Indonesia' },
]

const SEGMENT_TIMESLICE_MS = 5_000

export async function listMicrophones(): Promise<MicrophoneDevice[]> {
  if (typeof navigator === 'undefined' || !navigator.mediaDevices) return []
  try {
    const devices = await navigator.mediaDevices.enumerateDevices()
    return devices
      .filter((d) => d.kind === 'audioinput')
      .map((d, i) => ({
        id: d.deviceId,
        label: d.label || `Microphone ${i + 1}`,
      }))
  } catch {
    return []
  }
}

function pickMimeType(): string {
  const candidates = ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg;codecs=opus']
  for (const type of candidates) {
    if (typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported(type)) {
      return type
    }
  }
  return 'audio/webm'
}

export type NarrationCaptureOptions = {
  language: string
  deviceId?: string
  onError?: (message: string) => void
}

type BufferedSegment = {
  blob: Blob
  startEpoch: number
  stopEpoch: number
}

export class NarrationCapture {
  private stream: MediaStream | null = null
  private recorder: MediaRecorder | null = null
  private segmentStart = 0
  private muted = false
  private readonly options: NarrationCaptureOptions
  private readonly segments: BufferedSegment[] = []

  constructor(options: NarrationCaptureOptions) {
    this.options = options
  }

  async start(): Promise<void> {
    if (typeof navigator === 'undefined' || !navigator.mediaDevices) {
      throw new Error('microphone capture is not available in this environment')
    }
    const constraints: MediaStreamConstraints = {
      audio: {
        echoCancellation: true,
        noiseSuppression: true,
        channelCount: 1,
        ...(this.options.deviceId ? { deviceId: { exact: this.options.deviceId } } : {}),
      },
    }
    this.stream = await navigator.mediaDevices.getUserMedia(constraints)
    this.launchRecorder()
  }

  private launchRecorder() {
    if (!this.stream) return
    const mimeType = pickMimeType()
    const recorder = new MediaRecorder(this.stream, {
      mimeType,
      audioBitsPerSecond: 24_000,
    })
    this.recorder = recorder
    this.segmentStart = Date.now()
    recorder.ondataavailable = (event) => {
      if (!event.data || event.data.size === 0) return
      const stopEpoch = Date.now()
      const startEpoch = this.segmentStart
      this.segmentStart = stopEpoch
      if (this.muted) return
      this.segments.push({ blob: event.data, startEpoch, stopEpoch })
    }
    recorder.onerror = () => {
      this.options.onError?.('the microphone stopped unexpectedly')
    }
    recorder.start(SEGMENT_TIMESLICE_MS)
  }

  setMuted(muted: boolean) {
    this.muted = muted
    if (this.stream) {
      for (const track of this.stream.getAudioTracks()) {
        track.enabled = !muted
      }
    }
  }

  isMuted(): boolean {
    return this.muted
  }

  async stop(): Promise<void> {
    const recorder = this.recorder
    this.recorder = null
    if (recorder && recorder.state !== 'inactive') {
      await new Promise<void>((resolve) => {
        recorder.onstop = () => resolve()
        try {
          recorder.stop()
        } catch {
          resolve()
        }
      })
    }
    if (this.stream) {
      for (const track of this.stream.getTracks()) {
        track.stop()
      }
      this.stream = null
    }
  }

  hasSegments(): boolean {
    return this.segments.length > 0
  }

  async flush(recordingName: string): Promise<void> {
    const pending = this.segments.splice(0, this.segments.length)
    for (const segment of pending) {
      try {
        await uploadAudioSegment(recordingName, segment.blob, {
          language: this.options.language,
          startEpoch: segment.startEpoch,
          stopEpoch: segment.stopEpoch,
        })
      } catch (err) {
        this.options.onError?.(err instanceof Error ? err.message : String(err))
      }
    }
  }

  discard(): void {
    this.segments.length = 0
  }
}
