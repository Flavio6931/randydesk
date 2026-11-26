# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.5`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.5` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.5
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.5/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Probar en vivo `RandyDesk v0.4.5` en ambas PCs: barra flotante (`Pantalla` / `Acciones` / `Teclado` / `Desconectar`), tarjeta flotante de métricas y tecla `Win` con el cursor sobre el escritorio remoto.

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Apartado flotante superior estilo RustDesk (`v0.4.5`): `session_bar.rs` + `app.rs` con menús `Pantalla` (escala adaptativa/original, calidad de imagen, códec JPEG/LZ4, mostrar calidad del monitor, pantalla completa), `Acciones` (Win, `Ctrl+Shift+Esc`, portapapeles, refrescar), `Teclado` (`Modo traducción` / `Modo mapa`, toggle hook y contador en vivo) y botón rojo `Desconectar`.
- Fix definitivo tecla `Win` y foco Win32 (`v0.4.5`): `win_hook.rs` captura también cuando `cursor_pid == own_pid` (superando `fg_pid = SearchHost.exe` en Windows 11); `tray.rs` fuerza el foco con `AttachThreadInput` + `BringWindowToTop` + `SetForegroundWindow` (`ensure_foreground`).
- Verificación completa `v0.4.5`: `clippy` (0 warnings), `cargo test --workspace` (34/34 aprobadas), `cargo build --workspace --release` (`RandyDesk.exe` `11,340,800` bytes en `dist\`, `randydesk-relay.exe` `2,766,336` bytes) y Release `v0.4.5`.
- Pipeline de video paralelo/adaptativo (`encoder.rs` / `capture.rs` / `LatestFrameSlot`) y bandeja `EnumWindows` + `SW_HIDE`/`SW_RESTORE` (`v0.4.4`).
- Teclado híbrido estilo RustDesk + inyección `enigo` (`v0.4.3`), detección UIPI / `Ejecutar como Admin` (`v0.4.2`).
