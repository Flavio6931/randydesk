# Registro de Sesiones — RandyDesk

## 2026-11-24 — RandyDesk v0.4.0: Bandeja, sin contraseña, tecla Win remota y optimización GPU/Red
- **Resumen técnico:**
  - Versión subida a `0.4.0` en `Cargo.toml` y añadida `tray-icon = "0.19"`.
  - Bandeja del sistema (`crates/randydesk-client/src/tray.rs`): al minimizar oculta la ventana manteniendo activa la recepción y las sesiones remotas.
  - Acceso sin contraseña por defecto (`host_panel.rs`, `session.rs`, `relay_agent.rs`, `app.rs`) con cifrado E2E X25519/ChaCha20 intacto y contraseña opcional oculta en `...`.
  - Hook Win32 `WH_KEYBOARD_LL` (`crates/randydesk-client/src/win_hook.rs`) + botón `Tecla Win` para capturar `Win` y atajos hacia el escritorio remoto cuando el puntero está sobre él.
  - Optimización de latencia y nitidez: `set_nodelay(true)` en `randydesk-relay`, `connect_async_with_config(..., true)` en host/cliente, `E2eCipher::split()` + `ws.split()` concurrentes, JPEG 4:4:4 al 85% por defecto (`encoder.rs`) y actualización parcial de textura GPU (`decoder.rs` + `tex.set_partial` en `app.rs`).
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 20 pruebas aprobadas (7 en `randydesk-client`, 3 en `randydesk-host`, 1 `quic_loopback`, 8 en `randydesk-proto`, 1 `relay_e2e`).
  - `cargo build --workspace --release`: generados `target\release\RandyDesk.exe` (`11,089,920` bytes) y `target\release\randydesk-relay.exe` (`2,766,336` bytes).
  - Publicado Release `v0.4.0` en `Flavio6931/randydesk` (y `git push` a `main` para redesplegar `randydesk-relay` en Render con `TCP_NODELAY`).
- **Pendiente de verificación en vivo:**
  - Validar bandeja, acceso directo, tecla `Win` y fluidez/nitidez en sesión real con `RandyDesk v0.4.0`.
