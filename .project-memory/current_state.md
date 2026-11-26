# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.5.0`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.5.0` — https://github.com/Flavio6931/randydesk/releases/tag/v0.5.0
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.5.0/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Probar en vivo `RandyDesk v0.5.0` en ambas PCs: instalar servicio `SYSTEM` con 1 clic (`Instalar servicio (SYSTEM)`), verificar control sobre UAC, pantalla de bloqueo y `Enviar Ctrl + Alt + Del (SAS)`.
- [ ] Probar códec `H.264 (Fluido / Video)` desde `Pantalla -> Códec` (o `Opciones` en el Lobby) y promoción automática a `QUIC directo` en misma red LAN / Tailscale.

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Servicio Windows `NT AUTHORITY\SYSTEM` + `SendSAS` + captura UAC/Winlogon (`v0.5.0`): `win_service.rs` (`RandyDeskService`, supervisor Session 0 -> consola activa con `CreateProcessAsUserW`, política `SoftwareSASGeneration = 3`, CLI `--service`/`--host-worker`/`--install-service`/`--uninstall-service`), `send_sas()`, cambio dinámico de desktop `sync_thread_to_input_desktop` + fallback GDI en `capture.rs` y gestión con 1 clic en `host_panel.rs`.
- Códec de video `H.264` inter-frame (`v0.5.0`): `h264_codec.rs` (`H264FrameEncoder` con OpenH264 `ScreenContentRealTime` + SIMD `BgraSliceU8`→I420), `FrameDecoder` en `decoder.rs`, selector H.264 en Lobby y `session_bar.rs`.
- Promoción automática Relay → QUIC directo LAN/Tailscale (`v0.5.0`): `HostMsg::DirectEndpoints`, sondeo no bloqueante `probe_direct_quic`, `SessionExit::SwitchToDirect` y fallback automático a Relay (`PROTOCOL_VERSION = 2`, compatible hacia atrás con `v1`).
- Verificación completa `v0.5.0`: `clippy` (0 warnings), `cargo test --workspace` (41/41 aprobadas), `cargo build --workspace --release` (`RandyDesk.exe` `12,302,848` bytes en `dist\`, `randydesk-relay.exe` `2,768,384` bytes) y Release `v0.5.0`.
- Fase 1 (`v0.4.6`): fix definitivo de escritura (`win_down` incondicional, fallback `SearchHost`, `AltGr`), barra flotante colapsable con pin y downscale rápido `75%`/`50%`.
- Apartado flotante superior estilo RustDesk y captura `cursor_pid == own_pid` (`v0.4.5`).
