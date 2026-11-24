# Registro de Sesiones — RandyDesk

## 2026-11-24 — Unificación en `RandyDesk.exe` y publicación en GitHub (`v0.1.0`)
- **Resumen técnico:**
  - Agregado `HostService` (`crates/randydesk-host/src/service.rs`) y contador `active_clients` con guardia RAII `ActiveClientGuard` en `crates/randydesk-host/src/session.rs`.
  - Unificada la UI en `crates/randydesk-client` (`[[bin]] name = "RandyDesk"`, `windows_subsystem = "windows"` en release) con panel lateral izquierdo **"Tu Escritorio"** (`host_panel.rs`) + panel central **"Controlar escritorio remoto"** y **"Mis Equipos guardados"** (`app.rs` + `keymap.rs`).
  - Inicializado Git en rama `main` con cuenta fija `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`), creado repositorio público `Flavio6931/randydesk` y publicado Release `v0.1.0` con `RandyDesk.exe`.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 12 pruebas aprobadas (3 en `randydesk-client`, 3 unitarias en `randydesk-host`, 1 integración `quic_loopback`, 5 en `randydesk-proto`).
  - `cargo build -p randydesk-client --release`: generado `target\release\RandyDesk.exe` y copiado a `dist\RandyDesk.exe` (`9,495,040` bytes).
  - `gh release view v0.1.0`: verificado asset `RandyDesk.exe` (`9,495,040` bytes) disponible públicamente.
- **Pendiente de verificación en vivo:**
  - Probar conexión real entre tu PC y la de tu compañero con `RandyDesk.exe` (cerrando antes `randydesk-host.exe` local si sigue ocupando el puerto UDP `21118`).
