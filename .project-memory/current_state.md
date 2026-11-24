# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Fase:** Publicado en GitHub (`v0.1.0`) con ejecutable descargable `RandyDesk.exe`
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Release v0.1.0:** https://github.com/Flavio6931/randydesk/releases/tag/v0.1.0
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.1.0/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Prueba visual en vivo con tu compañero ejecutando `RandyDesk.exe` (LAN o Tailscale)
- [ ] Fase 2 (opcional): mini-servidor de señalización/relay propio si se requiere conexión sin Tailscale

## Últimas Tareas Completadas
- Implementación de `HostService` embebible en `crates/randydesk-host/src/service.rs` con contador atómico de sesiones activas y detección de IP LAN / Tailscale (`100.64.0.0/10`).
- Refactorización de `crates/randydesk-client` en binario único GUI sin consola `RandyDesk.exe` (`host_panel.rs` estilo "Tu Escritorio" + `keymap.rs` + libreta "Mis Equipos" en `app.rs`).
- Verificación completa: `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings), `cargo test --workspace` (12/12 tests) y `cargo build -p randydesk-client --release`.
- Publicación en repositorio público `Flavio6931/randydesk` (rama `main`) y Release `v0.1.0` adjuntando `RandyDesk.exe` (`9,495,040` bytes).
