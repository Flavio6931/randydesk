# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.4.6`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Servidor Relay propio (Render Free):** `wss://randydesk-relay.onrender.com` (preconfigurado por defecto en `RandyDesk.exe`)
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.4.6` — https://github.com/Flavio6931/randydesk/releases/tag/v0.4.6
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.4.6/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Probar en vivo `RandyDesk v0.4.6` en ambas PCs: escritura normal/AltGr, fallback cuando `SearchHost.exe` retiene foreground, tecla `Win`, barra colapsable (`Ocultar`/`Fijar`), escala `75%`/`50%` en `Pantalla`, e inversión/intercambio/solo lectura en `Teclado y ratón`.
- [ ] Fase 2 (`v0.5.0`): Servicio de Windows `NT AUTHORITY\SYSTEM` (`CreateServiceW`, `WTSGetActiveConsoleSessionId`, `CreateProcessAsUserW`, `SendSAS` para `Ctrl+Alt+Del` y control UAC/LockScreen) + conexión directa LAN/Tailscale por ID y códec H.264.

## Ideas Futuras Guardadas (Alternativas \$0 para `randydesk-relay`)
1. **En tu propia casa (PC vieja, mini-PC o Raspberry Pi) — \$0:**
   - Ejecutar `randydesk-relay.exe` 24/7 + **DuckDNS** (dominio gratis) abriendo el puerto `21119` en el router, o con **Cloudflare Tunnel (`cloudflared`)** si hay CGNAT.
   - *Pros:* tráfico ilimitado y nunca se duerme.
   - *Contra:* depende de la luz e internet de casa.
2. **En Oracle Cloud *Always Free* (servidor Linux dedicado en la nube) — \$0:**
   - VM Linux permanente con IP pública fija y **10 TB/mes gratis** que nunca se duerme *(pide tarjeta solo para verificar identidad, cobra \$0)*.

## Últimas Tareas Completadas
- Fix definitivo de escritura, `win_down` y AltGr (`v0.4.6`): limpieza incondicional de `win_down` al soltar `Win` + resincronización con `GetAsyncKeyState`, fallback automático `should_hook_capture_all_keys` cuando `SearchHost.exe` retiene foreground con cursor encima, `SetActiveWindow`/`SetFocus` en `tray.rs`, soporte AltGr (`@`, `\`, `{}`) y log `[HOST-UNI]`.
- Barra colapsable y paridad de menús estilo RustDesk (`v0.4.6`): pestaña mínima superior (`Ocultar`/`Fijar`), escala personalizada (`50%..=200%`), `Resolución de transmisión` (`100%`/`75%`/`50%`), `Mostrar cursor remoto`, `Bloquear pantalla remota` (`LockWorkStation`), `Invertir rueda`, `Intercambiar botones` y `Solo visualización`.
- Downscale rápido en host (`v0.4.6`): `TileEncoder::encode_scaled` (promedio 2x2 al 50% y punto fijo 16.16) + control de congestión en `relay_agent.rs` (`ClientMsg::SetStreamScale` y `LockWorkStation` compatibles hacia atrás).
- Verificación completa `v0.4.6`: `clippy` (0 warnings), `cargo test --workspace` (37/37 aprobadas), `cargo build --workspace --release` (`RandyDesk.exe` `11,395,072` bytes en `dist\`, `randydesk-relay.exe` `2,766,336` bytes) y Release `v0.4.6`.
- Apartado flotante superior estilo RustDesk y captura `cursor_pid == own_pid` (`v0.4.5`).
- Pipeline de video paralelo/adaptativo (`encoder.rs` / `capture.rs` / `LatestFrameSlot`) y bandeja `EnumWindows` (`v0.4.4`).
