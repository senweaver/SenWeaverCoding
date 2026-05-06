// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! TLS and mutual TLS (mTLS) support for the gateway server.
//!
//! Builds a [`rustls::ServerConfig`] from the gateway TLS configuration,
//! optionally requiring client certificates verified against a trusted CA
//! with optional certificate pinning (SHA-256 fingerprint matching).

use crate::config::schema::{GatewayClientAuthConfig, GatewayTlsConfig};
use anyhow::{Context, Result};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub fn build_tls_acceptor(config: &GatewayTlsConfig) -> Result<TlsAcceptor> {
    let server_config = build_server_config(config)?;
    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

pub fn build_server_config(config: &GatewayTlsConfig) -> Result<rustls::ServerConfig> {
    let certs = load_certs(&config.cert_path).with_context(|| {
        format!(
            "failed to load server certificate from {}",
            config.cert_path
        )
    })?;
    let key = load_private_key(&config.key_path)
        .with_context(|| format!("failed to load private key from {}", config.key_path))?;

    let client_auth_config = config.client_auth.as_ref().filter(|ca| ca.enabled);

    let builder = rustls::ServerConfig::builder();

    let server_config = if let Some(client_auth) = client_auth_config {
        let verifier = build_client_verifier(client_auth)
            .context("failed to build client certificate verifier")?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .context("invalid server certificate or key")?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .context("invalid server certificate or key")?
    };

    Ok(server_config)
}

fn build_client_verifier(config: &GatewayClientAuthConfig) -> Result<Arc<dyn ClientCertVerifier>> {
    let ca_certs = load_certs(&config.ca_cert_path)
        .with_context(|| format!("failed to load CA certificate from {}", config.ca_cert_path))?;

    let mut root_store = RootCertStore::empty();
    for cert in &ca_certs {
        root_store
            .add(cert.clone())
            .context("failed to add CA certificate to root store")?;
    }

    let base_verifier = if config.require_client_cert {
        WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .context("failed to build WebPKI client verifier")?
    } else {
        WebPkiClientVerifier::builder(Arc::new(root_store))
            .allow_unauthenticated()
            .build()
            .context("failed to build WebPKI client verifier (optional auth)")?
    };

    if config.pinned_certs.is_empty() {
        Ok(base_verifier)
    } else {
        let normalized: Vec<String> = config
            .pinned_certs
            .iter()
            .map(|fp| fp.replace(':', "").to_lowercase())
            .collect();
        Ok(Arc::new(PinnedCertVerifier {
            inner: base_verifier,
            pinned_fingerprints: normalized,
        }))
    }
}

pub fn cert_sha256_fingerprint(cert_der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert_der);
    let hash = hasher.finalize();
    hex::encode(hash)
}

#[derive(Debug)]
struct PinnedCertVerifier {
    inner: Arc<dyn ClientCertVerifier>,
    pinned_fingerprints: Vec<String>,
}

impl ClientCertVerifier for PinnedCertVerifier {
    fn offer_client_auth(&self) -> bool {
        self.inner.offer_client_auth()
    }

    fn client_auth_mandatory(&self) -> bool {
        self.inner.client_auth_mandatory()
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {

        self.inner
            .verify_client_cert(end_entity, intermediates, now)?;

        let fingerprint = cert_sha256_fingerprint(end_entity.as_ref());
        if self.pinned_fingerprints.contains(&fingerprint) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "client certificate fingerprint {fingerprint} is not in the pinned set"
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("cannot open certificate file: {path}"))?;
    let mut reader = std::io::BufReader::new(file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to parse PEM certificates from {path}"))?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in {path}");
    }
    Ok(certs)
}

fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("cannot open private key file: {path}"))?;
    let mut reader = std::io::BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)
        .with_context(|| format!("failed to parse private key from {path}"))?
        .ok_or_else(|| anyhow::anyhow!("no private key found in {path}"))?;
    Ok(key)
}
