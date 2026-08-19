// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::share::types::ShareWire;

pub const KIND_CONTROL: u8 = 1;
pub const KIND_FILE_CHUNK: u8 = 2;

pub const FILE_CHUNK_HEADER_LEN: usize = 16 + 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub nickname: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    #[serde(rename = "protocol")]
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ControlMessage {
    Chat {
        id: String,
        #[serde(rename = "tsMs")]
        ts_ms: i64,
        body: String,
    },
    FileOffer {
        #[serde(rename = "transferId")]
        transfer_id: String,
        name: String,
        #[serde(rename = "isDir")]
        is_dir: bool,
        #[serde(rename = "totalSize")]
        total_size: u64,
        #[serde(rename = "shareId", default, skip_serializing_if = "String::is_empty")]
        share_id: String,
    },
    FileComplete {
        #[serde(rename = "transferId")]
        transfer_id: String,
        #[serde(rename = "totalSize", default)]
        total_size: u64,
    },
    FileAbort {
        #[serde(rename = "transferId")]
        transfer_id: String,
        reason: String,
    },
    Ack {
        id: String,
    },
    ShareListRequest,
    ShareListResponse {
        shares: Vec<ShareWire>,
    },
    ShareDownloadRequest {
        #[serde(rename = "shareId")]
        share_id: String,
    },
}

pub async fn write_frame<W>(writer: &mut W, bytes: &[u8]) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let len = u32::try_from(bytes.len())?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(bytes).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R>(reader: &mut R, max_bytes: usize) -> Result<Vec<u8>>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > max_bytes {
        bail!("lan frame too large: {len} > {max_bytes}");
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

pub fn encode_control(message: &ControlMessage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.push(KIND_CONTROL);
    out.extend_from_slice(&serde_json::to_vec(message)?);
    Ok(out)
}

pub fn encode_file_chunk(transfer_id: &uuid::Uuid, offset: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + FILE_CHUNK_HEADER_LEN + data.len());
    out.push(KIND_FILE_CHUNK);
    out.extend_from_slice(transfer_id.as_bytes());
    out.extend_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(data);
    out
}

pub enum DecodedFrame {
    Control(ControlMessage),
    FileChunk {
        transfer_id: uuid::Uuid,
        offset: u64,
        data: Vec<u8>,
    },
}

pub fn decode_frame(plaintext: &[u8]) -> Result<DecodedFrame> {
    let Some((kind, body)) = plaintext.split_first() else {
        bail!("empty lan frame");
    };
    match *kind {
        KIND_CONTROL => {
            let message: ControlMessage = serde_json::from_slice(body)?;
            Ok(DecodedFrame::Control(message))
        }
        KIND_FILE_CHUNK => {
            if body.len() < FILE_CHUNK_HEADER_LEN {
                bail!("file chunk frame too short");
            }
            let mut id_bytes = [0u8; 16];
            id_bytes.copy_from_slice(&body[..16]);
            let transfer_id = uuid::Uuid::from_bytes(id_bytes);
            let mut offset_bytes = [0u8; 8];
            offset_bytes.copy_from_slice(&body[16..24]);
            let offset = u64::from_be_bytes(offset_bytes);
            let data = body[FILE_CHUNK_HEADER_LEN..].to_vec();
            Ok(DecodedFrame::FileChunk {
                transfer_id,
                offset,
                data,
            })
        }
        other => bail!("unknown lan frame kind: {other}"),
    }
}
