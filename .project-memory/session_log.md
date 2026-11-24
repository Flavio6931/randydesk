# Registro de Sesiones — RandyDesk

## 2026-11-24 — RandyDesk v0.3.0: IDs de 9 dígitos por Internet (Relay E2E) + Fixes visuales
- **Resumen técnico:**
  - Versión subida a `0.3.0` en `Cargo.toml` y añadido `crates/randydesk-relay` al workspace.
  - Cifrado extremo a extremo real (`crates/randydesk-proto/src/e2e.rs`) con X25519 + ChaCha20-Poly1305 y contadores monotónicos anti-replay, más IDs de 9 dígitos (`generate_host_id`, `format_host_id`, `parse_numeric_id`).
  - Servidor Relay WebSocket zero-knowledge (`crates/randydesk-relay`) con endpoint `/health` para Render/Cloudflare, más `Dockerfile` y `render.yaml` (`plan: free`).
  - Transporte dual en `randydesk-host` (`relay_agent.rs`) y `randydesk-client` (`relay_client.rs`, `settings.json`): ID numérico conecta vía Relay E2E `wss://`; IP conecta directo por QUIC LAN.
  - Fixes visuales en `host_panel.rs` y `app.rs`: PIN completo de 8 caracteres con botones debajo, barra de sesión remota sin solapamiento de botones y modal "Configurar servidor Relay".
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 18 pruebas aprobadas (6 en `randydesk-client`, 3 en `randydesk-host`, 1 `quic_loopback`, 7 en `randydesk-proto`, 1 `relay_e2e`).
  - `cargo build --workspace --release`: generados `dist\RandyDesk.exe` (`10,789,376` bytes) y `dist\randydesk-relay.exe` (`2,765,824` bytes).
  - Publicado Release `v0.3.0` en `Flavio6931/randydesk` con ambos binarios.
- **Pendiente de verificación en vivo:**
  - Probar conexión por Internet entre tu PC y la de tu compañero ejecutando `randydesk-relay.exe` + `cloudflared tunnel --url http://127.0.0.1:21119` (o desplegando en Render Free).
