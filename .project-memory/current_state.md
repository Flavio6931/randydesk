# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.0`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.0` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.0
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.0/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Probar `RandyDesk v0.4.0` en vivo con tu compañero (bandeja al minimizar, 1 clic sin contraseña, tecla Windows remota y fluidez/nitidez JPEG 4:4:4)

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Bandeja del sistema (`tray.rs` con `tray-icon 0.19`): al minimizar oculta la ventana manteniendo activa la sesión y el host en segundo plano.
- Acceso directo en 1 clic sin contraseña por defecto (`host_panel.rs`, `session.rs`, `relay_agent.rs`, `app.rs`), manteniendo cifrado E2E y contraseña opcional en `...`.
- Hook de teclado Win32 `WH_KEYBOARD_LL` (`win_hook.rs`) + botón `Tecla Win` para enviar `Win`, `Win+R/E/D` y `Alt+Tab` a la PC remota sin abrir el Menú Inicio local.
- Optimización de latencia y nitidez: `TCP_NODELAY` en `randydesk-relay` y WebSockets, `ws.split()` + `E2eCipher::split()`, JPEG 4:4:4 al 85%, y actualización parcial GPU (`tex.set_partial`).
- Verificación completa: `clippy` (0 warnings), `cargo test --workspace` (20/20 tests), `cargo build --workspace --release` (`11,089,920` bytes) y Release `v0.4.0`.
