# Registro de Sesiones — RandyDesk

## 2026-11-24 — RandyDesk v0.3.1: Servidor Relay fijo por defecto + Anti-suspensión Windows
- **Resumen técnico:**
  - Verificado el servidor propio del usuario en Render (`https://randydesk-relay.onrender.com/health` → `200 OK — RandyDesk Relay OK`).
  - Subida la versión del workspace a `0.3.1` y fijado `DEFAULT_RELAY_URL = "wss://randydesk-relay.onrender.com"` en `randydesk-proto` y `AppSettings` (`crates/randydesk-client/src/config.rs`).
  - Agregado `crates/randydesk-host/src/power.rs` usando `Win32_System_Power::SetThreadExecutionState` para evitar la suspensión automática de Windows durante la escucha Relay y sesiones activas.
  - Mejorado el modal Relay en `app.rs` con botones **"Usar por defecto"** y **"Solo red local"**.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 19 pruebas aprobadas (7 en `randydesk-client`, 3 en `randydesk-host`, 1 `quic_loopback`, 7 en `randydesk-proto`, 1 `relay_e2e`).
  - `cargo build --workspace --release`: generados `dist\RandyDesk.exe` (`10,780,160` bytes) y `dist\randydesk-relay.exe` (`2,765,824` bytes).
  - Publicado Release `v0.3.1` en `Flavio6931/randydesk`.
- **Pendiente de verificación en vivo:**
  - Probar conexión directa por ID de 9 dígitos entre tu PC y la de tu compañero abriendo `RandyDesk.exe v0.3.1` sin configurar nada.
