// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::Mutex;
use std::sync::Arc;

use anyhow::Result;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, Transport};
use tracing::{info, warn};

use crate::config::MqttConfig;
use crate::sop::audit::SopAuditLogger;
use crate::sop::dispatch::{dispatch_sop_event, process_headless_results};
use crate::sop::engine::{now_iso8601, SopEngine};
use crate::sop::types::{SopEvent, SopTriggerSource};

pub async fn run_mqtt_sop_listener(
    config: &MqttConfig,
    engine: Arc<Mutex<SopEngine>>,
    audit: Arc<SopAuditLogger>,
) -> Result<()> {
    config.validate()?;

    let mut mqtt_options = MqttOptions::new(
        &config.client_id,
        broker_host(&config.broker_url),
        broker_port(&config.broker_url),
    );
    mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keep_alive_secs));

    if let (Some(ref user), Some(ref pass)) = (&config.username, &config.password) {
        mqtt_options.set_credentials(user, pass);
    }

    if config.use_tls {
        mqtt_options.set_transport(Transport::tls_with_default_config());
        info!("MQTT SOP listener: TLS transport enabled");
    }

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 64);

    let qos = match config.qos {
        0 => QoS::AtMostOnce,
        1 => QoS::AtLeastOnce,
        _ => QoS::ExactlyOnce,
    };

    for topic in &config.topics {
        client.subscribe(topic, qos).await?;
        info!("MQTT SOP listener: subscribed to '{topic}'");
    }

    crate::health::mark_component_ok("mqtt");

    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(msg))) => {
                let topic = msg.topic.clone();
                let payload = String::from_utf8_lossy(&msg.payload).to_string();

                let event = SopEvent {
                    source: SopTriggerSource::Mqtt,
                    topic: Some(topic),
                    payload: Some(payload),
                    timestamp: now_iso8601(),
                };

                let results = dispatch_sop_event(&engine, &audit, event).await;
                process_headless_results(&results).await;
            }
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                crate::health::mark_component_ok("mqtt");
                info!("MQTT SOP listener: connected to broker");
            }
            Ok(_) => {

            }
            Err(e) => {
                crate::health::mark_component_error("mqtt", e.to_string());
                warn!("MQTT SOP listener: connection error: {e}");

            }
        }
    }
}

fn broker_host(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("mqtt://")
        .or_else(|| url.strip_prefix("mqtts://"))
        .unwrap_or(url);
    without_scheme
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string()
}

fn broker_port(url: &str) -> u16 {
    let is_tls = url.starts_with("mqtts://");
    let without_scheme = url
        .strip_prefix("mqtt://")
        .or_else(|| url.strip_prefix("mqtts://"))
        .unwrap_or(url);
    let default_port: u16 = if is_tls { 8883 } else { 1883 };
    without_scheme
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(default_port)
}
