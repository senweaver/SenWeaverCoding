// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VecExtError {
    #[error("rusqlite was not built with the `load_extension` feature")]
    NotSupportedBuild,
    #[error("could not locate a sqlite-vec shared library in search paths: {0:?}")]
    ExtensionNotFound(Vec<PathBuf>),
    #[error("sqlite error while loading extension: {0}")]
    Sqlite(String),
}

pub fn candidate_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Ok(custom) = std::env::var("SEN_VEC0_PATH") {
        paths.push(PathBuf::from(custom));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in platform_filenames() {
                paths.push(dir.join(name));
            }
        }
    }

    for name in platform_filenames() {
        paths.push(PathBuf::from(name));
    }

    paths
}

fn platform_filenames() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["sqlite-vec.dll", "vec0.dll", "sqlite_vec.dll"]
    } else if cfg!(target_os = "macos") {
        &["libsqlite-vec.dylib", "vec0.dylib", "libsqlite_vec.dylib"]
    } else {
        &["libsqlite-vec.so", "vec0.so", "libsqlite_vec.so"]
    }
}

#[cfg(feature = "vector-index-sqlite")]
pub fn try_load_sqlite_vec(conn: &rusqlite::Connection) -> Result<(), VecExtError> {

    #[cfg(not(feature = "rusqlite-load-extension"))]
    {
        let _ = conn;
        return Err(VecExtError::NotSupportedBuild);
    }

    #[cfg(feature = "rusqlite-load-extension")]
    {
        use rusqlite::LoadExtensionGuard;

        let _guard = unsafe { LoadExtensionGuard::new(conn) }
            .map_err(|e| VecExtError::Sqlite(e.to_string()))?;

        let paths = candidate_paths();
        let mut attempted = Vec::new();
        for path in &paths {

            let is_bare = path.components().count() == 1;
            if !is_bare && !path.exists() {
                continue;
            }
            attempted.push(path.clone());

            match unsafe { conn.load_extension(path, Some("sqlite3_vec_init")) } {
                Ok(_) => {
                    tracing::info!(
                        target: "memory.vector",
                        extension_path = %path.display(),
                        "loaded sqlite-vec extension"
                    );
                    return Ok(());
                }
                Err(e) => {
                    tracing::debug!(
                        target: "memory.vector",
                        extension_path = %path.display(),
                        error = %e,
                        "sqlite-vec load attempt failed; trying next candidate"
                    );
                }
            }
        }
        Err(VecExtError::ExtensionNotFound(attempted))
    }
}

#[cfg(not(feature = "vector-index-sqlite"))]
pub fn try_load_sqlite_vec(_conn: &rusqlite::Connection) -> Result<(), VecExtError> {
    Err(VecExtError::NotSupportedBuild)
}

pub fn create_vec0_table(conn: &rusqlite::Connection, dim: usize) -> Result<(), VecExtError> {

    let sql = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories_hnsw \
         USING vec0(embedding float[{dim}])"
    );
    conn.execute_batch(&sql)
        .map_err(|e| VecExtError::Sqlite(e.to_string()))
}
