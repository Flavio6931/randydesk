# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.4`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.4` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.4
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.4/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Cerrar `RandyDesk.exe` en ejecución (PID `6060`) y copiar `target\release\RandyDesk.exe` (`11,263,488` bytes) → `dist\RandyDesk.exe`
- [ ] Actualizar ambas PCs a `RandyDesk v0.4.4` y probar en vivo: restauración desde bandeja (clic, doble clic y menú), fluidez de video y perfiles (`Rápido`/`Equilibrado`/`Nítido`), y `VK_LWIN`/`VK_RWIN` con diagnóstico en `input_debug.log`

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Pipeline de video y red sin lag acumulado (`v0.4.4`): codificación paralela de tiles con `std::thread::scope`, submuestreo JPEG 4:2:0 (`quality < 85`) / 4:4:4 (`>= 85`), calidad adaptativa en movimiento (`>35%` tiles `-16`, piso `42` y refresco nítido posterior) en `encoder.rs`; reutilización de buffer BGRA + drenaje DXGI en `capture.rs`; reemplazo de cola por `LatestFrameSlot` en `session.rs` y `relay_agent.rs`.
- Fix definitivo de bandeja del sistema (`v0.4.4`): `TrayController` en `tray.rs` y `app.rs` usan `EnumWindows` + `IsIconic` + `ShowWindow(SW_HIDE/SW_RESTORE)` + `SetForegroundWindow` (sin `ViewportCommand::Visible(false)` que congelaba `eframe/winit`).
- Hook dinámico de teclado (`v0.4.4`): `KeyboardHookGuard` instala/desinstala `WH_KEYBOARD_LL` bajo demanda en `set_capture()` estilo RustDesk (`windows.cc`) con log de `VK_LWIN`/`VK_RWIN` en `input_debug.log` (`win_hook.rs`).
- Perfiles rápidos estilo RustDesk (`v0.4.4`): `Rápido` (JPEG 52, 30 FPS), `Equilibrado` (JPEG 72, 30 FPS por defecto en `message.rs` y `app.rs`) y `Nítido` (LZ4 sin pérdida) más slider de calidad JPEG.
- Verificación completa `v0.4.4`: `clippy` (0 warnings), `cargo test --workspace` (33/33 aprobadas), `cargo build --workspace --release` (`RandyDesk.exe` `11,263,488` bytes, `randydesk-relay.exe` `2,766,336` bytes) y Release `v0.4.4`.
- Arquitectura híbrida de teclado estilo RustDesk (`v0.4.3`): `egui` siempre activo para texto/navegación, sincronización `Ctrl/Alt/Shift`, `WH_KEYBOARD_LL` limitado a atajos de sistema e inyección `enigo` (`wVk + wScan`, `input.rs`).
- Detección UIPI + botón `Ejecutar como Admin` (`v0.4.2`), bandeja del sistema y acceso directo sin contraseña (`v0.4.0`).
