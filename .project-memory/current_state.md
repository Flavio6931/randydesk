# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.8.0`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.8.0` — https://github.com/Flavio6931/randydesk/releases/tag/v0.8.0
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.8.0/RandyDesk.exe
- **Diseño Figma (`Obsidian Craft`):** https://www.figma.com/design/8cPEueVwbe7tsD2wJ5Gq6G
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Actualizar la app en 1 clic a `v0.8.0` (`Buscar` -> `Actualizar a v0.8.0`) y probar en vivo gestor dual de archivos, Drag & Drop y audio WASAPI remoto (`Audio`).
- [ ] **Fase 3 (`v0.9.0`):** P2P por Internet (UDP Hole Punching), H.264 GPU (Media Foundation) y Multi-sesión por pestañas + estado *Online/Offline* real.

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- **Fase 2 (`v0.8.0`):** Transferencia bidireccional de archivos (`128 KiB`, validación anti-traversal, `.rdpart` atómico, gestor dual + Drag & Drop en `file_manager.rs` / `files.rs`) y audio remoto en vivo WASAPI Loopback + reproductor PCM `48 kHz` con compresión pura IMA-ADPCM 4:1 (`audio_codec.rs`, `host/audio.rs`, `client/audio.rs`, menú `Audio` y botón `Archivos` en `session_bar.rs`).
- **Fase 1 (`v0.7.0`):** Portapapeles automático bidireccional anti-eco (`GetClipboardSequenceNumber` + `ClipboardWatcher`), soporte multi-monitor DXGI/GDI en vivo (`SelectMonitor`/`MonitorList`), cursor remoto dinámico (`GetCursorInfo` -> `CursorShape`), botones laterales del ratón (`Extra1`/`Extra2` -> `MOUSEEVENTF_XDOWN`/`XUP`) y logs de diagnóstico de teclado desactivados por defecto.
- Pulido visual definitivo **Obsidian Craft (`v0.6.1`)** en `egui`.
- Rediseño visual completo **Obsidian Craft** en Figma (`RandyDesk — UI Redesign`) y portado inicial a `egui` (`v0.6.0`).
- Fix definitivo de `Modo mapa (Teclado físico)` estilo RustDesk (`REA + Ghidra`) y Auto-Actualizador en 1 clic (`v0.5.3`).
- Captura estricta de teclado por foco de ventana de `RandyDesk` y `Modo mapa` por defecto (`v0.5.2`).
- Fix definitivo de teclado cliente + host `SYSTEM` estilo RustDesk (`v0.5.1`).
- Servicio Windows `NT AUTHORITY\SYSTEM` + `SendSAS`, códec `H.264` inter-frame y promoción automática Relay → QUIC directo (`v0.5.0`).
