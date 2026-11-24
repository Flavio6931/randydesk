# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.2.0`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.2.0` — https://github.com/Flavio6931/randydesk/releases/tag/v0.2.0
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.2.0/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Cerrar `dist\RandyDesk.exe` (v0.1.0 abierto en sesión local) y copiar `target\release\RandyDesk.exe` → `dist\RandyDesk.exe`
- [ ] Prueba visual en vivo con tu compañero usando `RandyDesk v0.2.0` (LAN o Tailscale)
- [ ] Fase 2 (opcional): mini-servidor de señalización/relay propio si se requiere conexión sin Tailscale

## Últimas Tareas Completadas
- Rediseño visual completo estilo RustDesk (`v0.2.0`): tema oscuro limpio (`theme.rs`), barra lateral minimalista "Tu escritorio" (`host_panel.rs`), cuadrícula de tarjetas de colores con logotipo vectorial Windows (`cards.rs`), modales de contraseña/editor/guía (`app.rs`), y barra inferior de estado.
- Modelo `SavedHost` retrocompatible con `favorite`, `last_connected` y `color_idx`, más `upsert_recent` y `display_address` (`config.rs`).
- Verificación completa: `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings), `cargo test --workspace` (14/14 tests) y `cargo build -p randydesk-client --release`.
- Publicación de `v0.2.0` en `Flavio6931/randydesk` con `RandyDesk.exe` (`9,692,160` bytes).
