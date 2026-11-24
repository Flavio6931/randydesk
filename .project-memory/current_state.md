# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.3`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.3` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.3
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.3/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Cerrar `dist\RandyDesk.exe` (`v0.4.2` en ejecución) y copiar `target\release\RandyDesk.exe` → `dist\RandyDesk.exe`
- [ ] Actualizar ambas PCs a `RandyDesk v0.4.3` y probar en vivo texto normal, atajos `Ctrl/Alt/Shift`, tecla `Win` y `Atajos Win: ON`

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Arquitectura híbrida de teclado estilo RustDesk (`v0.4.3`): `egui` siempre activo para texto/navegación, sincronización `Ctrl/Alt/Shift` (`app.rs`), y `WH_KEYBOARD_LL` limitado a atajos de sistema (`Win`, `Win+R/E/D`, `Alt+Tab`, `Alt+Esc`, `Ctrl+Esc`, `PrintScreen`) con botón `Atajos Win: ON (N)` (`win_hook.rs`).
- Inyección estilo `enigo` (`wVk + wScan` sin `KEYEVENTF_SCANCODE`), separación `down/up` de `35 ms` en botón `Tecla Win`, fix de handle `GetThreadDesktop` en `ensure_input_desktop()` y log `%APPDATA%\RandyDesk\input_debug.log` (`input.rs`).
- Verificación completa `v0.4.3`: `clippy` (0 warnings), `cargo test --workspace` (24/24 tests), `cargo build --workspace --release` (`11,174,400` bytes) y Release `v0.4.3`.
- Detección UIPI + botón `Ejecutar como Admin` (`v0.4.2`), bandeja del sistema y acceso directo sin contraseña (`v0.4.0`).
