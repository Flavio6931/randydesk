# Registro de Sesiones — RandyDesk

## 2026-11-26 — RandyDesk v0.4.5: Barra flotante estilo RustDesk y fix definitivo de foco / tecla `Win`
- **Resumen técnico:**
  - Versión subida a `0.4.5` en `Cargo.toml` (y `Cargo.lock`).
  - `crates/randydesk-client/src/win_hook.rs`: corregido bloqueo en Windows 11 donde `fg_pid` correspondía a `SearchHost.exe`; ahora `should_capture_for_pids(own_pid, fg_pid, cursor_pid)` captura también cuando `cursor_pid == own_pid` (`GetCursorPos` + `WindowFromPoint` + `GetAncestor(GA_ROOT)`). Añadido enum `KeyboardMode` (`Translate` y `Map`), `set_mode()` e `is_capturing_all_keys()`.
  - `crates/randydesk-client/src/tray.rs`: implementado `force_foreground` con `AttachThreadInput` + `BringWindowToTop` + `SetForegroundWindow` en `restore_window` y nuevo método público `TrayController::ensure_foreground()`.
  - `crates/randydesk-client/src/session_bar.rs`, `main.rs` y `app.rs`: reemplazada la barra fija superior por el apartado flotante centrado estilo RustDesk (`Pantalla`, `Acciones`, `Teclado`, `Desconectar`) más tarjeta flotante de métricas (`Mostrar calidad del monitor`), soporte `Escala adaptativa` / `Escala original` (`ScrollArea::both()`), protección para que clics en la barra/menús no pasen al escritorio remoto, y sincronización de foco `ensure_foreground()` al hacer clic dentro del canvas.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 34 pruebas aprobadas (15 en `randydesk-client`, 9 unit + 1 `quic_loopback` en `randydesk-host`, 8 en `randydesk-proto`, 1 `relay_e2e` en `randydesk-relay`).
  - `cargo build --workspace --release` + copia a `dist\`: `RandyDesk.exe` (`11,340,800` bytes) y `randydesk-relay.exe` (`2,766,336` bytes).
  - Publicado commit en `main` y Release `v0.4.5` en `Flavio6931/randydesk`.
- **Pendiente de verificación en vivo:**
  - Probar `v0.4.5` en ambas PCs (barra flotante, métricas, restauración desde bandeja y tecla `Win` sobre el escritorio remoto).

## 2026-11-24 — RandyDesk v0.4.4: Fluidez de video/red, fix bandeja Win32, hook dinámico y perfiles rápidos
- **Resumen técnico:**
  - Versión subida a `0.4.4` en `Cargo.toml` (y `Cargo.lock`).
  - `crates/randydesk-host/src/encoder.rs`: codificación paralela de tiles modificados con `std::thread::scope`, submuestreo JPEG `4:2:0` (`SamplingFactor::F_2_2`) cuando `quality < 85` y `4:4:4` (`R_4_4_4`) cuando `quality >= 85`, más calidad adaptativa en movimiento (`>35%` de tiles modificados reduce temporalmente `-16` con piso `42` y refresco nítido al cesar el movimiento).
  - `crates/randydesk-host/src/capture.rs`: reutilización de buffer `bgra: Vec<u8>` vía `recycle_buffer` y drenaje de hasta 8 cuadros acumulados DXGI (`AcquireNextFrame(0)`) cerrando cada `DxgiFrameGuard` entre llamadas.
  - `crates/randydesk-host/src/session.rs` y `relay_agent.rs`: reemplazo de `mpsc::channel(2)` por `Arc<LatestFrameSlot>` compartido para enviar siempre el cuadro más fresco sin lag acumulado, fusionando tiles y preservando `full_refresh` en keyframes pendientes.
  - `crates/randydesk-client/src/tray.rs` y `app.rs`: localizado y cacheado de `HWND` principal con `EnumWindows`, detección de minimizado con `IsIconic` + guarda `>= 450 ms`, ocultamiento directo con `ShowWindow(SW_HIDE)` (eliminando `ViewportCommand::Visible(false)` que congelaba `eframe/winit`) y restauración directa Win32 (`ShowWindow(SW_RESTORE)` + `SetForegroundWindow`).
  - `crates/randydesk-client/src/win_hook.rs`: ciclo dinámico de `WH_KEYBOARD_LL` en `set_capture()` estilo RustDesk (`windows.cc`) sin instalar hook al arranque, y registro de `VK_LWIN`/`VK_RWIN` en `input_debug.log`.
  - `crates/randydesk-proto/src/message.rs` y `crates/randydesk-client/src/app.rs`: calidad por defecto `72` (`StreamSettings::default` y `quality_input`) y 3 perfiles rápidos en la barra de sesión: `Rápido` (JPEG 52, 30 FPS), `Equilibrado` (JPEG 72, 30 FPS) y `Nítido` (LZ4 sin pérdida), conservando el slider JPEG.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 33 pruebas aprobadas (14 en `randydesk-client`, 9 unit + 1 `quic_loopback` en `randydesk-host`, 8 en `randydesk-proto`, 1 `relay_e2e` en `randydesk-relay`).
  - `cargo build --workspace --release`: generado `target\release\RandyDesk.exe` (`11,263,488` bytes) y `target\release\randydesk-relay.exe` (`2,766,336` bytes, copiado a `dist\`). `dist\RandyDesk.exe` (`11,174,400` bytes) no se sobreescribió porque `RandyDesk.exe` (PID `6060`) seguía en ejecución.
  - Publicado commit en `main` y Release `v0.4.4` en `Flavio6931/randydesk`.
- **Pendiente de verificación en vivo:**
  - Cerrar `RandyDesk.exe` (PID `6060`), copiar `target\release\RandyDesk.exe` a `dist\`, actualizar ambas PCs a `v0.4.4` y validar restauración desde bandeja, fluidez/perfiles y `VK_LWIN`/`VK_RWIN`.

## 2026-11-24 — RandyDesk v0.4.3: Teclado híbrido estilo RustDesk (`egui` + hook solo sistema + `enigo` VK)
- **Resumen técnico:**
  - Versión subida a `0.4.3` en `Cargo.toml`.
  - Aislada causa raíz tras prueba en vivo: con el modo `egui` (`KEYEVENTF_UNICODE` / `wVk`) la escritura sí funcionaba, mientras que secuestrar todo el teclado con `WH_KEYBOARD_LL` + `KEYEVENTF_SCANCODE` bloqueaba letras normales y Menú Inicio.
  - `crates/randydesk-client/src/win_hook.rs`: hook `WH_KEYBOARD_LL` limitado exclusivamente a teclas de sistema al estilo RustDesk (`windows.cc`: `VK_LWIN`/`VK_RWIN`, combos con `Win`, `Alt+Tab`, `Alt+Esc`, `Ctrl+Esc`, `PrintScreen`), dejando pasar teclas normales al flujo de ventana `egui`.
  - `crates/randydesk-client/src/app.rs`: flujo `egui::Event::Text` y `egui::Event::Key` siempre activo, sincronización de modificadores `Ctrl`/`Alt`/`Shift` con la PC remota, botón `Atajos Win: ON (N)` y retardo de `35 ms` entre `down` y `up` en el botón `Tecla Win`.
  - `crates/randydesk-host/src/input.rs`: inyección estilo `enigo` (`wVk + wScan` sin `KEYEVENTF_SCANCODE` cuando `vk != 0`), corrección en `ensure_input_desktop()` para no cerrar el handle de `GetThreadDesktop`, y log de diagnóstico en `%APPDATA%\RandyDesk\input_debug.log`.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 24 pruebas aprobadas (9 en `randydesk-client`, 5 en `randydesk-host`, 1 `quic_loopback`, 8 en `randydesk-proto`, 1 `relay_e2e`).
  - `cargo build --workspace --release`: generado `target\release\RandyDesk.exe` (`11,174,400` bytes) y `target\release\randydesk-relay.exe` (`2,766,336` bytes, copiado a `dist\`). `dist\RandyDesk.exe` no se sobreescribió porque `RandyDesk.exe` (`v0.4.2`, PID `10888`) seguía en ejecución.
  - Publicado commit en `main` y Release `v0.4.3` en `Flavio6931/randydesk`.
- **Pendiente de verificación en vivo:**
  - Cerrar `dist\RandyDesk.exe` (PID `10888`), copiar `target\release\RandyDesk.exe` a `dist\`, actualizar ambas PCs a `v0.4.3` y probar escritura, atajos, `Win` y `Atajos Win: ON`.

## 2026-11-24 — RandyDesk v0.4.2: Inyección por scancode estilo RustDesk, desktop activo y UIPI/Admin
- **Resumen técnico:**
  - Comparado con la arquitectura real de RustDesk (`libs/enigo/src/win/win_impl.rs`, `src/server/input_service.rs`, `src/platform/windows.cc` y `rustdesk-org/rdev/src/windows/{simulate,common,grab}.rs`).
  - Versión subida a `0.4.2` en `Cargo.toml` agregando features `Win32_Security`, `Win32_System_StationsAndDesktops` y `Win32_UI_Shell`.
  - Rediseñado `crates/randydesk-host/src/input.rs`: hilo dedicado FIFO `randydesk-input-injector`, adjunto dinámico al escritorio de entrada (`OpenInputDesktop` + `SetThreadDesktop`), inyección `KEYEVENTF_SCANCODE` con `wVk = 0`, resolución `MapVirtualKeyExW(..., MAPVK_VK_TO_VSC_EX, layout_foreground)`, reglas `0x36`/`VK_PACKET`, `RANDYDESK_INPUT_EXTRA_INFO`, detección UIPI (`is_process_elevated`, `is_foreground_window_elevated`, `uipi_block_detected`) y `relaunch_as_admin()`.
  - Endurecido `crates/randydesk-client/src/win_hook.rs`: `PeekMessageW` previo al hook, `GetMessageW(...).0 > 0`, `try_lock()` no bloqueante, filtro propio por `dwExtraInfo` y `release_modifiers_now()` sin apagar el canal.
  - Actualizados `app.rs`, `service.rs` y `host_panel.rs`: estado `Teclado: Fallback` si falla el hook, scan code `0x5B` explícito en botón `Tecla Win`, y badge/botón `Ejecutar como Admin` en barra lateral.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 23 pruebas aprobadas (8 en `randydesk-client`, 5 en `randydesk-host`, 1 `quic_loopback`, 8 en `randydesk-proto`, 1 `relay_e2e`).
  - `cargo build --workspace --release` + copia a `dist\`: `RandyDesk.exe` (`11,136,512` bytes) y `randydesk-relay.exe` (`2,766,336` bytes).
  - Publicado commit en `main` y Release `v0.4.2` en `Flavio6931/randydesk`.
- **Pendiente de verificación en vivo:**
  - Actualizar ambas PCs a `v0.4.2` y verificar teclado, atajos, `Win` y control de ventanas elevadas con `Ejecutar como Admin`.

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
