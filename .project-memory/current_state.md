# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.6.1`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.6.1` — https://github.com/Flavio6931/randydesk/releases/tag/v0.6.1
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.6.1/RandyDesk.exe
- **Diseño Figma (`Obsidian Craft`):** https://www.figma.com/design/8cPEueVwbe7tsD2wJ5Gq6G
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Actualizar de `v0.6.0` a `v0.6.1` en 1 clic desde la app (`Buscar actualización` -> `Actualizar a v0.6.1`) y validar visualmente los 6 ajustes en pantalla completa (`1920x1080`).
- [ ] Probar en vivo `Modo mapa (Teclado físico · Recomendado)`, códec `H.264` y promoción `Relay -> QUIC directo`.

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Pulido visual definitivo **Obsidian Craft (`v0.6.1`)** en `egui`: fuente nativa `Segoe UI`, grid por filas fijas `272×152` sin efecto escalera (`cards.rs`), degradado vertical continuo por malla (`paint_vertical_gradient_banner`), inputs `42px`/`34px` con borde visible y centrado vertical (`styled_text_input`), píldoras opacas (`PILL_*` en `theme.rs`, `host_panel.rs`, `session_bar.rs`), ancho máximo `1180px` en `app.rs` y botón `...` de una sola capa.
- Rediseño visual completo **Obsidian Craft** en Figma (`RandyDesk — UI Redesign`) y portado inicial a `egui` (`v0.6.0`).
- Fix definitivo de `Modo mapa (Teclado físico)` estilo RustDesk (`REA + Ghidra`) y Auto-Actualizador en 1 clic (`v0.5.3`): inyección física pura por scan code (`KEYEVENTF_SCANCODE`, `wVk = 0`) con filtro `0x021D` (`AltGr`), bloqueo local solo en `KeyDown`, `AllowSetForegroundWindow` y auto-actualizador GitHub Releases (`updater.rs`).
- Captura estricta de teclado por foco de ventana de `RandyDesk` y `Modo mapa` por defecto (`v0.5.2`).
- Fix definitivo de teclado cliente + host `SYSTEM` estilo RustDesk (`v0.5.1`).
- Servicio Windows `NT AUTHORITY\SYSTEM` + `SendSAS`, códec `H.264` inter-frame y promoción automática Relay → QUIC directo (`v0.5.0`).
