# Registro de Sesiones — RandyDesk

## 2026-11-24 — RandyDesk v0.4.1: Replicación completa de teclado y fix visual `□`
- **Resumen técnico:**
  - Versión subida a `0.4.1` en `Cargo.toml` y añadida feature `Win32_System_LibraryLoader`.
  - Corregido `crates/randydesk-client/src/win_hook.rs`: pasa `GetModuleHandleW(None)` explícito a `SetWindowsHookExW`, valida `installed` real antes de suprimir eventos `egui`, fuerza `extended = true` en `VK_LWIN`/`VK_RWIN`, y captura mientras la ventana está en primer plano sin requerir que el mouse esté sobre el canvas.
  - Mejorado `crates/randydesk-host/src/input.rs`: resuelve `MapVirtualKeyW(vk, MAPVK_VK_TO_VSC)` cuando `scan == 0` y fuerza `KEYEVENTF_EXTENDEDKEY` en `VK_LWIN`/`VK_RWIN` (con test unitario `win_keys_always_include_extended_flag`).
  - Añadido toggle `Teclado: ON/OFF` en `app.rs` y reemplazados caracteres `●`/`○` por `painter().circle_filled(...)` en `app.rs` y `host_panel.rs`.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 21 pruebas aprobadas (7 en `randydesk-client`, 4 en `randydesk-host`, 1 `quic_loopback`, 8 en `randydesk-proto`, 1 `relay_e2e`).
  - `cargo build --workspace --release`: generado `target\release\RandyDesk.exe` (`11,092,992` bytes) y `target\release\randydesk-relay.exe` (`2,766,336` bytes). *(Nota: `dist\RandyDesk.exe` estaba en uso por el usuario y no se sobreescribió localmente).*
  - Publicado Release `v0.4.1` en `Flavio6931/randydesk`.
- **Pendiente de verificación en vivo:**
  - Cerrar `dist\RandyDesk.exe` (incluida bandeja del sistema), abrir `target\release\RandyDesk.exe` (`v0.4.1`) y probar escritura/atajos/`Win` en sesión remota.
