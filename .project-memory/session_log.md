# Registro de Sesiones — RandyDesk

## 2026-11-24 — Unificación en binario único gráfico `RandyDesk.exe`
- **Resumen técnico:**
  - Agregado `HostService` (`crates/randydesk-host/src/service.rs`) y contador `active_clients` con guardia RAII `ActiveClientGuard` en `crates/randydesk-host/src/session.rs`.
  - Unificada la UI en `crates/randydesk-client` (`[[bin]] name = "RandyDesk"`, `windows_subsystem = "windows"` en release) con panel lateral izquierdo **"Tu Escritorio"** (`host_panel.rs`) que auto-inicia la recepción en UDP `21118`, muestra IP LAN/Tailscale y PIN/contraseña editable, junto al panel central **"Controlar escritorio remoto"** y **"Mis Equipos guardados"** (`app.rs` + `keymap.rs`).
  - Generada copia portable en `dist\RandyDesk.exe`.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 12 pruebas aprobadas (3 en `randydesk-client`, 3 unitarias en `randydesk-host`, 1 integración `quic_loopback`, 5 en `randydesk-proto`).
  - `cargo build -p randydesk-client --release`: generado `target\release\RandyDesk.exe` y copiado a `dist\RandyDesk.exe` (`9,495,040` bytes). *(Nota: `cargo build --workspace --release` omitió sobreescribir `target\release\randydesk-host.exe` porque el proceso PID 19532 estaba ejecutándose en el equipo).*
- **Pendiente de verificación en vivo:**
  - Cerrar `randydesk-host.exe` si sigue abierto, ejecutar `dist\RandyDesk.exe` con doble clic y validar captura DXGI, entrada y portapapeles en sesión real.
