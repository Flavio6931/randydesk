//! Servicio embebible del agente host (QUIC directo LAN + agente Relay E2E por Internet).

use crate::capture;
use crate::relay_agent;
use crate::session::{accept_loop, HostServerConfig};
use crate::tls;
use anyhow::{Context, Result};
use quinn::Endpoint;
use randydesk_proto::{normalize_relay_ws_url, StreamSettings};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

/// Resumen vivo del estado del servidor host para mostrar en UI.
#[derive(Debug, Clone)]
pub struct HostRuntimeInfo {
    pub running: bool,
    pub host_id: String,
    pub port: u16,
    pub lan_ip: Option<String>,
    pub tailscale_ip: Option<String>,
    pub relay_url: Option<String>,
    pub relay_connected: bool,
    pub fingerprint_sha256: String,
    pub active_clients: usize,
    pub is_elevated: bool,
    pub is_system: bool,
    pub uipi_warning: bool,
}

/// Controlador activo del servidor QUIC y del agente Relay del host embebido.
pub struct HostService {
    endpoint: Endpoint,
    task: JoinHandle<()>,
    relay_task: Option<JoinHandle<()>>,
    relay_stop: Arc<AtomicBool>,
    relay_connected: Arc<AtomicBool>,
    active_clients: Arc<AtomicUsize>,
    host_id: String,
    port: u16,
    lan_ip: Option<String>,
    tailscale_ip: Option<String>,
    relay_url: Option<String>,
    fingerprint_sha256: String,
    stopped: bool,
}

impl HostService {
    /// Inicia el servidor QUIC LAN y, si se indica `relay_url`, registra el ID en el servidor Relay.
    pub fn start(
        rt: &Handle,
        port: u16,
        password: String,
        monitor: usize,
        settings: StreamSettings,
        relay_url: Option<String>,
    ) -> Result<Self> {
        let identity = tls::load_or_create_server_identity()?;
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        // Entrar al reactor Tokio es obligatorio si se invoca desde el hilo de UI.
        let _guard = rt.enter();
        let endpoint = Endpoint::server(identity.server_config, bind_addr)
            .with_context(|| format!("no se pudo abrir puerto UDP {port}"))?;
        let actual_port = endpoint.local_addr()?.port();

        let active_clients = Arc::new(AtomicUsize::new(0));
        let host_name =
            std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows-Host".to_string());
        let lan_ip = detect_lan_ip();
        let tailscale_ip = detect_tailscale_ip();
        let mut direct_endpoints = Vec::new();
        if let Some(ref ts) = tailscale_ip {
            direct_endpoints.push(format!("{ts}:{actual_port}"));
        }
        if let Some(ref lan) = lan_ip {
            direct_endpoints.push(format!("{lan}:{actual_port}"));
        }

        let cfg = HostServerConfig {
            password,
            host_name,
            default_settings: settings,
            capturer_factory: Arc::new(move || capture::create_capturer(monitor)),
            active_clients: Arc::clone(&active_clients),
            direct_endpoints,
        };

        let host_id = relay_agent::load_or_create_host_id();
        let normalized_relay = relay_url.and_then(|u| normalize_relay_ws_url(&u));
        let relay_stop = Arc::new(AtomicBool::new(false));
        let relay_connected = Arc::new(AtomicBool::new(false));

        let task = rt.spawn(accept_loop(endpoint.clone(), cfg.clone()));
        let relay_task = normalized_relay.as_ref().map(|url| {
            rt.spawn(relay_agent::run_relay_loop(
                url.clone(),
                host_id.clone(),
                cfg,
                Arc::clone(&relay_connected),
                Arc::clone(&relay_stop),
            ))
        });

        Ok(Self {
            endpoint,
            task,
            relay_task,
            relay_stop,
            relay_connected,
            active_clients,
            host_id,
            port: actual_port,
            lan_ip,
            tailscale_ip,
            relay_url: normalized_relay,
            fingerprint_sha256: identity.fingerprint_sha256,
            stopped: false,
        })
    }

    /// Devuelve la información actual de red, ID numérico, estado Relay y sesiones conectadas.
    pub fn info(&self) -> HostRuntimeInfo {
        HostRuntimeInfo {
            running: !self.stopped && !self.task.is_finished(),
            host_id: self.host_id.clone(),
            port: self.port,
            lan_ip: self.lan_ip.clone(),
            tailscale_ip: self.tailscale_ip.clone(),
            relay_url: self.relay_url.clone(),
            relay_connected: !self.stopped && self.relay_connected.load(Ordering::Relaxed),
            fingerprint_sha256: self.fingerprint_sha256.clone(),
            active_clients: self.active_clients.load(Ordering::Relaxed),
            is_elevated: crate::input::is_process_elevated(),
            is_system: crate::win_service::is_running_as_system(),
            uipi_warning: crate::input::uipi_block_detected(),
        }
    }

    /// Detiene tanto el endpoint QUIC como la conexión saliente al servidor Relay.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.relay_stop.store(true, Ordering::Relaxed);
        self.relay_connected.store(false, Ordering::Relaxed);
        self.endpoint.close(0u32.into(), b"host detenido");
        self.task.abort();
        if let Some(t) = self.relay_task.take() {
            t.abort();
        }
    }
}

impl Drop for HostService {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Detecta la IP local LAN sin enviar paquetes reales a Internet.
pub fn detect_lan_ip() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

/// Detecta la IP de Tailscale (rango CGNAT `100.64.0.0/10`) consultando la tabla de rutas local.
pub fn detect_tailscale_ip() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("100.100.100.100:53").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if is_tailscale_ipv4(v4) => Some(v4.to_string()),
        _ => None,
    }
}

/// Comprueba si una IPv4 pertenece al rango CGNAT `100.64.0.0/10` usado por Tailscale.
pub fn is_tailscale_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0xC0) == 64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_tailscale_cgnat_range() {
        assert!(is_tailscale_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_tailscale_ipv4(Ipv4Addr::new(100, 100, 55, 12)));
        assert!(is_tailscale_ipv4(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_tailscale_ipv4(Ipv4Addr::new(100, 63, 255, 255)));
        assert!(!is_tailscale_ipv4(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_tailscale_ipv4(Ipv4Addr::new(192, 168, 1, 10)));
    }

    #[test]
    fn host_service_starts_and_stops_on_ephemeral_port() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        let mut svc = HostService::start(
            rt.handle(),
            0,
            "clave-prueba".to_string(),
            0,
            StreamSettings::default(),
            None,
        )
        .expect("iniciar HostService");

        let info = svc.info();
        assert!(info.running);
        assert!(info.port > 0);
        assert_eq!(info.host_id.len(), 9);
        assert_eq!(info.active_clients, 0);
        assert!(!info.fingerprint_sha256.is_empty());

        svc.stop();
        assert!(!svc.info().running);
    }
}
