# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.5.3`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.5.3` — https://github.com/Flavio6931/randydesk/releases/tag/v0.5.3
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.5.3/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Descargar `RandyDesk.exe v0.5.3` por última vez en ambas PCs (desde `v0.5.4+` ya se actualiza con el botón integrado `Actualizar a vX.Y.Z` / `Buscar actualización`).
- [ ] Probar en vivo `Modo mapa (Teclado físico · Recomendado)` en `v0.5.3`: escribir, usar `Win`, `Win+R/E`, `Alt+Tab`, `AltGr`, confirmar que no escape a tu PC dentro de RandyDesk y que al tocar la barra de tareas local regrese tu teclado.
- [ ] Probar el botón `Buscar actualización` / `Actualizar a vX.Y.Z`, códec `H.264` y promoción automática `Relay -> QUIC directo`.

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Fix definitivo de `Modo mapa (Teclado físico)` estilo RustDesk (`REA + Ghidra`) y Auto-Actualizador en 1 clic (`v0.5.3`): inyección física pura por scan code (`KEYEVENTF_SCANCODE`, `wVk = 0`) con filtro `0x021D` (`AltGr`) y exclusión `0x36`/`0x45`, bloqueo local solo en `KeyDown` (pasando `KeyUp`/`CapsLock`/`NumLock` a `CallNextHookEx`), liberación completa de teclas pulsadas con doble pulso `Alt`/`AltGr`, `AllowSetForegroundWindow` en `tray.rs`, más auto-actualizador integrado desde GitHub Releases en `updater.rs` (`--apply-update` + `--cleanup-file` + actualización de `RandyDeskService`).
- Captura estricta de teclado por foco de ventana de `RandyDesk` y `Modo mapa` por defecto (`v0.5.2`): foco automático al conectar en `app.rs`, eliminación de captura por `cursor_pid` en `win_hook.rs` (`should_capture_for_focus`), arreglo de `Win+R`/`Win+E` en `Modo traducción`, e indicador claro en `session_bar.rs`.
- Fix definitivo de teclado cliente + host `SYSTEM` estilo RustDesk (`v0.5.1`): token interactivo de `winlogon.exe` en `win_service.rs` (`DETACHED_PROCESS`), logger asíncrono sin I/O en `WH_KEYBOARD_LL` + log `SYSTEM` en `C:\ProgramData\RandyDesk\host_input_debug.log`, fix `desktop_names_match` + `GENERIC_WRITE` + liberación de `Shift` en `KEYEVENTF_UNICODE` en `input.rs`, refresco de canal `same_channel` en `win_hook.rs`, fix `AltGr` (`VK_RMENU`) y reenvío de `Copy`/`Cut`/`Paste` (`Ctrl+C/X/V`) en `app.rs`.
- Servicio Windows `NT AUTHORITY\SYSTEM` + `SendSAS` + captura UAC/Winlogon (`v0.5.0`): `win_service.rs`, `send_sas()`, cambio dinámico de desktop y gestión con 1 clic en `host_panel.rs`.
- Códec de video `H.264` inter-frame (`v0.5.0`): `h264_codec.rs`, `FrameDecoder` en `decoder.rs` y selector H.264.
- Promoción automática Relay → QUIC directo LAN/Tailscale (`v0.5.0`): `HostMsg::DirectEndpoints`, `probe_direct_quic` y `SwitchToDirect` (`PROTOCOL_VERSION = 2`).
- Fase 1 (`v0.4.6`): fix escritura (`win_down` incondicional, fallback `SearchHost`, `AltGr`), barra flotante colapsable con pin y downscale rápido `75%`/`50%`.
