# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.1`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.1` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.1
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.1/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Cerrar `dist\RandyDesk.exe` (v0.4.0 en ejecución) y copiar `target\release\RandyDesk.exe` → `dist\RandyDesk.exe`
- [ ] Probar en vivo `RandyDesk v0.4.1` con tu compañero (replicación completa de teclado con botón `Teclado: ON` y tecla `Win`)

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Corrección de replicación de teclado (`win_hook.rs` con `GetModuleHandleW`, captura activa en primer plano sin depender de hover sobre canvas, `input.rs` con `MapVirtualKeyW` y `KEYEVENTF_EXTENDEDKEY` en `VK_LWIN`/`VK_RWIN`, y toggle `Teclado: ON/OFF`).
- Reemplazo de `●`/`○` por círculos vectoriales (`circle_filled`) eliminando los cuadros `□` en `host_panel.rs` y `app.rs`.
- Verificación completa: `clippy` (0 warnings), `cargo test --workspace` (21/21 tests), `cargo build --workspace --release` (`11,092,992` bytes) y Release `v0.4.1`.
