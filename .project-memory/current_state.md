# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.3.1`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.3.1` — https://github.com/Flavio6931/randydesk/releases/tag/v0.3.1
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.3.1/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Prueba visual en vivo con tu compañero usando `RandyDesk v0.3.1` (cero configuración, directo por ID de 9 dígitos)

## Últimas Tareas Completadas
- Fijado `wss://randydesk-relay.onrender.com` como `DEFAULT_RELAY_URL` en `randydesk-proto` y `AppSettings` (`config.rs`, con soporte `relay_disabled` y botones `Usar por defecto` / `Solo red local` en `app.rs`).
- Agregado módulo anti-suspensión de Windows `crates/randydesk-host/src/power.rs` (`SetThreadExecutionState` con `ES_SYSTEM_REQUIRED` y `ES_DISPLAY_REQUIRED`) para evitar cortes en sesiones largas de 2–24+ horas.
- Verificación completa: `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings), `cargo test --workspace` (19/19 tests), `cargo build --workspace --release` (`dist\RandyDesk.exe` `10,780,160` bytes) y Release `v0.3.1` publicado.
