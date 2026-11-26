# RandyDesk — Contexto del Proyecto

- **Tipo:** Aplicación de escritorio remoto para Windows en Rust puro (estilo RustDesk, sin límites de tiempo).
- **Versión actual:** `0.5.0` (regla del repo: cada mejora sube `[workspace.package].version` en `Cargo.toml` y publica Release `vX.Y.Z` en `Flavio6931/randydesk` con `RandyDesk.exe`).
- **Stack principal:** Rust 2021, `eframe`/`egui 0.29`, `quinn` (QUIC + TLS 1.3 TOFU), `tokio-tungstenite` + X25519/ChaCha20-Poly1305 (Relay E2E), `openh264` (H.264 Annex-B), `jpeg-encoder`/`zune-jpeg`, `lz4_flex`, `windows 0.58` (DXGI/GDI, `SendInput`, `WH_KEYBOARD_LL`, SCM `LocalSystem`, `SendSAS`).

## Arquitectura por Crates
- `crates/randydesk-proto`: protocolo binario (`bincode` con límite 32 MiB), framing, auth SHA-256 HMAC, cifrado E2E X25519+ChaCha20 (`PROTOCOL_VERSION = 2`, compatible desde `1`).
- `crates/randydesk-host`: agente anfitrión embebible y de servicio:
  - `capture.rs`: DXGI Desktop Duplication con drenaje y fallback GDI + seguimiento de escritorio activo (`Default`/`Winlogon`).
  - `encoder.rs` & `h264_codec.rs`: codificación incremental por tiles (JPEG/LZ4, downscale 75%/50%) y streaming H.264 `ScreenContentRealTime`.
  - `input.rs` & `power.rs`: hilo FIFO `SendInput` (`enigo` VK+scan) adjunto al Input Desktop activo, detección UIPI, `LockWorkStation`, `SendSAS`.
  - `win_service.rs`: servicio `RandyDeskService` (`NT AUTHORITY\SYSTEM`) en `C:\Program Files\RandyDesk\`, estado en `C:\ProgramData\RandyDesk\`, supervisor Session 0 -> consola activa.
  - `session.rs` & `relay_agent.rs`: servidor QUIC LAN, agente saliente Relay E2E (`wss://randydesk-relay.onrender.com`) y anuncio de `DirectEndpoints`.
- `crates/randydesk-client`: GUI unificada (`RandyDesk.exe`), bandeja Win32 (`tray.rs`), hook teclado (`win_hook.rs`), barra flotante colapsable (`session_bar.rs`), decodificador GPU (`decoder.rs`), promoción automática Relay → QUIC directo (`net.rs`, `relay_client.rs`) y panel del servicio `SYSTEM` (`host_panel.rs`).
- `crates/randydesk-relay`: servidor WebSocket Rendezvous + puente E2E zero-knowledge (`randydesk-relay.exe`).

## Reglas Técnicas del Proyecto
- Textos de interfaz siempre en español sin símbolos Unicode rotos en fuentes de `egui` (usar `painter().circle_filled(...)` / `Shape::convex_polygon(...)`).
- Mantener compatibilidad `bincode`: agregar variantes nuevas solo al final de `ClientMsg`, `HostMsg` y `CodecKind`.
- En Windows PowerShell 5.1 encadenar comandos con `; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }` (no usar `&&`).
- Verificar siempre con `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y `cargo build --workspace --release` tras la última edición.
