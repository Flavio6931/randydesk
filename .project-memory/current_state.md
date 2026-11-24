# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.2`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.2` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.2
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.2/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Actualizar ambas PCs a `RandyDesk v0.4.2` y probar en vivo escritura/atajos/`Win` (y botón `Ejecutar como Admin` si controlas SSMS, Terminal Admin o UAC)

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Inyección de teclado por hardware scan code (`KEYEVENTF_SCANCODE` + `wVk = 0`, `MapVirtualKeyExW` sobre `HKL` de la ventana activa) y trabajador FIFO dedicado adjunto al Input Desktop activo (`OpenInputDesktop` / `SetThreadDesktop`) al estilo RustDesk (`input.rs`).
- Endurecimiento del hook `WH_KEYBOARD_LL` en el cliente (`win_hook.rs` + `app.rs`): `PeekMessageW` previo, `try_lock()` no bloqueante, filtrado por `RANDYDESK_INPUT_EXTRA_INFO`, canal continuo en sesión y manejo de `VK_PACKET`/`RShift 0x36`.
- Detección de bloqueo UIPI y opción **Ejecutar como Admin** en el panel lateral (`service.rs`, `host_panel.rs`).
- Verificación completa: `clippy` (0 warnings), `cargo test --workspace` (23/23 tests), `cargo build --workspace --release` (`11,136,512` bytes), `dist\` actualizado y Release `v0.4.2`.
- Replicación completa previa (`v0.4.1`) y bandeja del sistema/acceso directo (`v0.4.0`).
