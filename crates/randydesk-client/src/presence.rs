//! Rastreador asincrono de presencia Online/Offline contra `GET /api/status?ids=...` del servidor Relay.

use randydesk_proto::parse_numeric_id;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct StatusResponse {
    #[serde(default)]
    online: Vec<String>,
}

/// Convierte una URL WebSocket del Relay (`ws://` o `wss://`) a base HTTP (`http://` o `https://`).
pub fn ws_to_http_base(ws_url: &str) -> Option<String> {
    let trimmed = ws_url.trim().trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("wss://") {
        Some(format!("https://{rest}"))
    } else {
        trimmed
            .strip_prefix("ws://")
            .map(|rest| format!("http://{rest}"))
    }
}

/// Consulta periodicamente en segundo plano el estado en linea de los IDs guardados.
pub struct PresenceTracker {
    rt_handle: tokio::runtime::Handle,
    online_ids: Arc<Mutex<HashSet<String>>>,
    has_data: Arc<std::sync::atomic::AtomicBool>,
    last_poll: Instant,
    in_flight: Arc<std::sync::atomic::AtomicBool>,
}

impl PresenceTracker {
    /// Inicializa un tracker ligado al runtime Tokio compartido.
    pub fn new(rt_handle: tokio::runtime::Handle) -> Self {
        Self {
            rt_handle,
            online_ids: Arc::new(Mutex::new(HashSet::new())),
            has_data: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_poll: Instant::now() - Duration::from_secs(60),
            in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Dispara un refresco asincrono cada 15 s si hay IDs numericos y URL Relay configurada.
    pub fn maybe_refresh(
        &mut self,
        relay_url: Option<&str>,
        addresses: &[String],
        ctx: &eframe::egui::Context,
    ) {
        let Some(ws_url) = relay_url else {
            return;
        };
        if self.last_poll.elapsed() < Duration::from_secs(15) {
            return;
        }
        if self
            .in_flight
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let Some(http_base) = ws_to_http_base(ws_url) else {
            self.in_flight
                .store(false, std::sync::atomic::Ordering::Release);
            return;
        };
        let ids: Vec<String> = addresses
            .iter()
            .filter_map(|a| parse_numeric_id(a))
            .collect();
        if ids.is_empty() {
            self.in_flight
                .store(false, std::sync::atomic::Ordering::Release);
            return;
        }

        self.last_poll = Instant::now();
        let online_ids = Arc::clone(&self.online_ids);
        let has_data = Arc::clone(&self.has_data);
        let in_flight = Arc::clone(&self.in_flight);
        let ctx_clone = ctx.clone();

        self.rt_handle.spawn(async move {
            let url = format!("{http_base}/api/status?ids={}", ids.join(","));
            if let Ok(client) = reqwest::Client::builder()
                .timeout(Duration::from_secs(4))
                .build()
            {
                if let Ok(resp) = client.get(&url).send().await {
                    if resp.status().is_success() {
                        if let Ok(body) = resp.text().await {
                            if let Ok(parsed) = serde_json::from_str::<StatusResponse>(&body) {
                                if let Ok(mut guard) = online_ids.lock() {
                                    guard.clear();
                                    guard.extend(parsed.online);
                                }
                                has_data.store(true, std::sync::atomic::Ordering::Release);
                                ctx_clone.request_repaint();
                            }
                        }
                    }
                }
            }
            in_flight.store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// Devuelve el mapa de presencia conocido (`Some(true)` en linea, `Some(false)` desconectado).
    pub fn snapshot(&self, addresses: &[String]) -> HashMap<String, bool> {
        let mut out = HashMap::new();
        if !self.has_data.load(std::sync::atomic::Ordering::Acquire) {
            return out;
        }
        let Ok(guard) = self.online_ids.lock() else {
            return out;
        };
        for addr in addresses {
            if let Some(id) = parse_numeric_id(addr) {
                out.insert(id.clone(), guard.contains(&id));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ws_and_wss_relay_urls_to_http() {
        assert_eq!(
            ws_to_http_base("wss://randydesk-relay.onrender.com/").as_deref(),
            Some("https://randydesk-relay.onrender.com")
        );
        assert_eq!(
            ws_to_http_base("ws://127.0.0.1:21119").as_deref(),
            Some("http://127.0.0.1:21119")
        );
        assert_eq!(ws_to_http_base("http://invalid"), None);
    }
}
