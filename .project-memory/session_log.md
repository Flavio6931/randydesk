# Registro de Sesiones — RandyDesk

## 2026-11-26 — RandyDesk v0.8.0 (Fase 2): Transferencia de archivos (gestor dual + Drag & Drop) y Audio remoto WASAPI
- **Resumen técnico:**
  - `Cargo.toml` (+ `Cargo.lock`): versión `0.8.0` y features `Win32_Media_Audio`, `Win32_System_Com`.
  - [audio_codec.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-proto/src/audio_codec.rs), [lib.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-proto/src/lib.rs) y [message.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-proto/src/message.rs): `PROTOCOL_VERSION = 4`, códec IMA-ADPCM estéreo 4:1 puro en Rust, `FILE_CHUNK_SIZE = 128 KiB`, variantes al final de `ClientMsg`/`HostMsg` (`SetAudioEnabled`, `FileListReq/Resp`, `FileUploadStart/Chunk/Finish`, `FileDownloadReq/Meta/Chunk`, `FileTransferDone/Error/Cancel`, `FileMkdir`, `AudioData`).
  - [audio.rs (host)](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/audio.rs) y [files.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/files.rs): captura WASAPI Loopback `48 kHz` estéreo con supresión de silencio (`AUDCLNT_BUFFERFLAGS_SILENT`), navegación segura (unidades Windows, `sanitize_file_name`), `.rdpart` con limpieza RAII en `Drop` y renombrado sin sobrescritura (`unique_dest_path`).
  - [session.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/session.rs) y [relay_agent.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/relay_agent.rs): worker de audio opcional (`client_version >= 4`), envío sin bloqueos y despacho compartido `HostFileManager::handle_client_msg`.
  - [audio.rs (client)](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/audio.rs), [file_manager.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/file_manager.rs), [net.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/net.rs), [relay_client.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/relay_client.rs), [session_bar.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/session_bar.rs) y [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/app.rs): reproductor WASAPI independiente del FPS de `egui`, ventana dual local/remota con progreso y cancelación, Drag & Drop directo sobre la sesión, controles `Archivos` y `Audio` en el HUD.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 55 pruebas aprobadas (23 `randydesk-client`, 20 unit + 1 `quic_loopback` `randydesk-host`, 10 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release` + Release `v0.8.0` en GitHub.

## 2026-11-26 — RandyDesk v0.7.0 (Fase 1): Portapapeles automático, Multi-monitor, Cursor dinámico, Botones extra y Logs bajo demanda
- **Resumen técnico:**
  - `Cargo.toml` (+ `Cargo.lock`): versión `0.7.0` y feature `Win32_System_DataExchange`.
  - [lib.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-proto/src/lib.rs) y [message.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-proto/src/message.rs): `PROTOCOL_VERSION = 3` (compatible `>= 1`), variantes al final para mantener compatibilidad `bincode` (`MouseButton::Extra1/Extra2`, `ClientMsg::SelectMonitor`, `HostMsg::MonitorList`, `HostMsg::CursorShape`), tipos `MonitorInfo` y `CursorKind`.
  - [clipboard.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/clipboard.rs): `sequence_number()` vía `GetClipboardSequenceNumber()`, `hash_text()` FNV-1a y `ClipboardWatcher` anti-eco con límite `256 KiB`.
  - [capture.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/capture.rs): `list_monitors()` DXGI/GDI con índices 1:1 y `query_system_cursor()` vía `GetCursorInfo` + `LoadCursorW` cacheados.
  - [input.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/input.rs): `build_mouse_button_input` con `MOUSEEVENTF_XDOWN/XUP` (`0x0001`/`0x0002`) y `INPUT_DEBUG_LOGGING` apagado por defecto (`set_input_debug_logging`, `RANDYDESK_INPUT_LOG`).
  - [session.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/session.rs), [relay_agent.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/relay_agent.rs), [service.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/service.rs), [main.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/main.rs): `CapturerFactory` parametrizada por monitor, cambio de monitor en caliente sin cortar sesión, streaming de cambios de cursor y sincronización automática de portapapeles en ambos transportes.
  - [keymap.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/keymap.rs), [net.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/net.rs), [relay_client.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/relay_client.rs), [session_bar.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/session_bar.rs), [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/app.rs): mapeo `Extra1/Extra2`, selector `Monitor` en `Pantalla`, checkbox de portapapeles automático en `Acciones`, toggle de log en `Teclado y ratón` y replicación `CursorIcon`.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 51 pruebas aprobadas (23 `randydesk-client`, 18 unit + 1 `quic_loopback` `randydesk-host`, 8 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release` + Release `v0.7.0` en GitHub (`RandyDesk.exe` y `randydesk-relay.exe`).

## 2026-11-26 — RandyDesk v0.6.1: Fix visual Obsidian Craft (`Segoe UI`, grid por filas, degradado suave y píldoras opacas)
- **Causa raíz visual observada en vivo (`v0.6.0`):**
  1. `ui.horizontal_wrapped` + `allocate_ui` generaba desplazamiento en escalera entre tarjetas vecinas.
  2. El rectángulo `lower_half` translúcido producía un corte horizontal duro a mitad del banner.
  3. `ui.add_sized(..., TextEdit::singleline(...))` dejaba cajas negras sin borde alineadas arriba a la izquierda.
  4. `from_rgba_unmultiplied` en `egui` lavaba las píldoras (`Relay E2E`, `UAC + SAS`, `Copiar`, `Desconectar`).
  5. Fuente bitmap por defecto de `egui` poco nítida y doble recuadro detrás del botón `...` de las tarjetas, además de hero sobre-estirado a `1920×1080`.
- **Resumen técnico:**
  - `Cargo.toml` (+ `Cargo.lock`): versión subida a `0.6.1`.
  - [theme.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/theme.rs): carga nativa de `C:\Windows\Fonts\segoeui.ttf`, constantes opacas `PILL_*`, `BANNER_GRADIENTS`, `paint_vertical_gradient_banner` (malla continua con esquinas redondeadas) y `styled_text_input` (borde visible + foco cobalto + centrado vertical).
  - [cards.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/cards.rs): grid determinista por filas (`chunks(cols)` con `272×152`, gap `16 px`), `allocate_exact_size`, banner con malla degradada, pill opaca superior y menú `...` de una sola capa.
  - [host_panel.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/host_panel.rs), [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/app.rs) y [session_bar.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydesk-client/src/session_bar.rs): píldoras opacas (`PILL_*`), `content_w` acotado a `1180 px` en `CentralPanel`, y campos `styled_text_input` de `42 px` (dirección) / `34 px` (búsqueda) / `30 px` (contraseña).
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 48 pruebas aprobadas (22 `randydesk-client`, 16 unit + 1 `quic_loopback` `randydesk-host`, 8 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release` + Release `v0.6.1` en GitHub (`target\release\RandyDesk.exe` `13,800,960` bytes; `dist\RandyDesk.exe` omitido sin matar el proceso abierto por el usuario).

## 2026-11-26 — RandyDesk v0.6.0: Rediseño UI Obsidian Craft (Figma -> egui)
- **Diseño en Figma:** https://www.figma.com/design/8cPEueVwbe7tsD2wJ5Gq6G (`01 · Design System` con variables/componentes, `02 · Lobby (Inicio)` 1240×740, `03 · Sesión Remota & HUD`).
- **Resumen técnico:**
  - `Cargo.toml` (+ `Cargo.lock`): versión subida a `0.6.0`.
  - [theme.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/theme.rs): tokens Obsidian Craft (`#0B0D11`, `#101319`, `#141821`, `#1B202C`, `#0D1016`, `#2F81F7`, `#10B981`, `#059669`, `#F59E0B`, `#FCA5A5`), bordes de 1 px (`#1F2533`/`#313B52`), banners profundos y helpers vectoriales (`draw_star`, `draw_lock_icon`, `draw_status_pill`).
  - [host_panel.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/host_panel.rs): sidebar `272 px` con tarjeta `"TU ID DE ACCESO"` (`Copiar`, `...`, `Relay E2E`, `LAN`, `Configurar PIN`), tarjeta dedicada `"Servicio SYSTEM"` (`UAC + SAS`) y sello inferior `Cifrado E2E · X25519 + ChaCha20`.
  - [cards.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/cards.rs): `HostCard` `256×148` (`12 px` radius), banner degradado oscuro con pill `Relay E2E`/`QUIC LAN`, estrella vectorial de favoritos y pie con nombre e ID/IP.
  - [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/app.rs): Hero ancho `"Controlar escritorio remoto"` con selector segmentado de códec (`H.264` / `JPEG` / `LZ4 Nítido`), pestañas segmentadas (`Recientes` / `Favoritos` / `Todos`), botón `+ Agregar` y footer con botón `Actualizar a vX.Y.Z`.
  - [session_bar.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydesk-client/src/session_bar.rs): barra flotante glass (`12 px`), pill en vivo de transporte (`QUIC`/`Relay`), botón `Desconectar` rojo translúcido y tarjeta de telemetría sin solapamiento (`y = 58`).
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 48 pruebas aprobadas (22 `randydesk-client`, 16 unit + 1 `quic_loopback` `randydesk-host`, 8 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release` + copia a `dist\`.

## 2026-11-26 — RandyDesk v0.5.3: Fix definitivo `Modo mapa (Teclado físico)` estilo RustDesk (`REA + Ghidra`) + Auto-Actualizador en 1 clic
- **Causa raíz confirmada con REA + Ghidra 12.2 (`RustDesk.exe` / `librustdesk.dll`) y fuente de RustDesk (`rdev` Windows + `keyboard.rs` + `input_service.rs`):**
  1. `Modo mapa` en `v0.5.2` enviaba `vk != 0` en lugar de scan code físico puro (`wVk = 0`, `KEYEVENTF_SCANCODE`), no filtraba el scan code sintético `0x021D` de `AltGr`, ni excluía `0x45` (`NumLock`/`Pause`) de `KEYEVENTF_EXTENDEDKEY`.
  2. `WH_KEYBOARD_LL` devolvía `LRESULT(1)` también en `WM_KEYUP`/`WM_SYSKEYUP` (a diferencia de RustDesk que bloquea solo `KeyDown` y siempre llama a `CallNextHookEx` en `KeyUp`, además de `CapsLock`/`NumLock`).
  3. Al perder foco en `Modo mapa` faltaba soltar todas las teclas físicas rastreadas (y hacer el doble pulso de `Alt`/`AltGr`), y `force_foreground` no usaba `AllowSetForegroundWindow(ASFW_ANY)`.
- **Resumen técnico:**
  - `Cargo.toml`, `crates/randydesk-client/Cargo.toml` (+ `Cargo.lock`): versión `0.5.3` y dependencia `reqwest 0.12` (`rustls-tls`, `stream`).
  - [win_hook.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/win_hook.rs): `build_map_key_event` (scan code puro `vk = 0`, filtro `0x021D`, exclusión `0x36`/`0x45`, `VK_PAUSE`/`VK_PACKET`), `should_block_locally` (solo `KeyDown`, deja pasar `KeyUp`, `CapsLock` y `NumLock`), seguimiento y liberación completa de `pressed_keys` (con doble pulso `Alt`/`AltGr`) y tests unitarios.
  - [input.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/input.rs): exclusión `0x45` junto a `0x36` para `KEYEVENTF_EXTENDEDKEY` y test `physical_scancode_mode_matches_rustdesk_rdev`.
  - [tray.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydesk-client/src/tray.rs) y [session_bar.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydesk-client/src/session_bar.rs): `AllowSetForegroundWindow(ASFW_ANY)` + `SW_SHOW` en `force_foreground` y etiqueta explícita de `Modo físico` vs `Modo traducción`.
  - [updater.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/updater.rs), [main.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/main.rs) y [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/app.rs): botón de actualización en 1 clic (`Buscar actualización` / `Actualizar a vX.Y.Z`) consultando GitHub Releases, descarga con progreso y validación PE (`MZ`), reemplazo seguro (`--apply-update`, UAC solo si hace falta y actualización de `RandyDeskService`), relanzamiento y limpieza en `%TEMP%` (`--cleanup-file`).
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 48 pruebas aprobadas (22 `randydesk-client`, 16 unit + 1 `quic_loopback` `randydesk-host`, 8 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release` + copia a `dist\`.

## 2026-11-26 — RandyDesk v0.5.2: Captura de teclado por foco estricto de `RandyDesk` y `Modo mapa` por defecto
- **Causa raíz identificada (video + `input_debug.log`):**
  1. `KeyboardMode::Translate` era el modo por defecto al conectar, dejando pasar teclas fuera del hook y rompiendo `Win+R`/`Win+E` por depender de `GetAsyncKeyState(VK_LWIN)` cuando el hook bloqueaba `Win`.
  2. La regla `cursor_pid == own_pid` hacía que el teclado siguiera secuestrado hacia la PC remota cuando otra ventana local (`OBS`, `SearchHost`, barra de tareas) tenía foco si el puntero permanecía sobre `RandyDesk`, y escapaba si el puntero tocaba la barra de tareas local.
  3. Faltaba enfoque automático al llegar el primer cuadro de la sesión remota.
- **Resumen técnico:**
  - `Cargo.toml` (+ `Cargo.lock`): versión subida a `0.5.2`.
  - [win_hook.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/win_hook.rs): `KeyboardMode::Map` como valor por defecto (`#[default]`), reemplazo de `cursor_pid` por `should_capture_for_focus(own_pid, fg_pid, window_focused)` (`set_window_focused`), limpieza de `is_cursor_over_own_window` y fix de `win_was_down` en `Modo traducción`.
  - [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/app.rs): enfoque automático único al conectar (`focused_on_connect` + `ensure_foreground()` + `ViewportCommand::Focus`) y sincronización continua de `set_window_focused(ctx.input(|i| i.focused))`.
  - [session_bar.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/session_bar.rs): `Modo mapa (Teclado físico · Recomendado)` primero en el menú y etiqueta clara de foco (`Teclado: PC remota (activo, N teclas)` vs `Teclado: Tu PC (haz clic aquí para controlar)`).
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 43 pruebas aprobadas (18 `randydesk-client`, 15 unit + 1 `quic_loopback` `randydesk-host`, 8 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release` + copia limpia a `dist\`: `RandyDesk.exe` (`12,352,512` bytes) y `randydesk-relay.exe` (`2,768,384` bytes).

## 2026-11-26 — RandyDesk v0.5.1: Fix definitivo de teclado cliente y host `SYSTEM` estilo RustDesk
- **Causa raíz identificada (comparando código RustDesk y trazas reales en Windows 11):**
  1. `win_service.rs`: `spawn_worker_in_session` duplicaba el token de Session 0 con `SecurityIdentification` + `TokenSessionId` + `CREATE_NO_WINDOW`, en lugar de usar el token interactivo de `winlogon.exe` de la sesión activa (`DETACHED_PROCESS`), bloqueando `INPUT_KEYBOARD` bajo Windows 11 aunque el ratón funcionaba.
  2. `input.rs`: `sync_thread_to_input_desktop()` no pedía `GENERIC_WRITE`, consideraba `(None, None)` como escritorios iguales sin cambiar al Input Desktop y enviaba `KEYEVENTF_UNICODE` con `Shift` potencialmente retenido; además el log del servicio iba a `systemprofile\AppData` y hacía `flush()` síncrono.
  3. `win_hook.rs`: `low_level_kb_proc` hacía escritura y `flush()` síncronos a disco dentro del callback `WH_KEYBOARD_LL` (riesgo de `LowLevelHooksTimeout`) y `set_capture` no actualizaba `state.tx` al cambiar de canal abierto.
  4. `app.rs` + `egui-winit 0.29.1`: `mods.command == mods.ctrl` en Windows anulaba `AltGr`, y `egui-winit` convertía `Ctrl+C/X/V` en `egui::Event::Copy/Cut/Paste` sin emitir `Event::Key`.
- **Resumen técnico:**
  - `Cargo.toml` (+ `Cargo.lock`): versión subida a `0.5.1` y agregada feature `Win32_System_Diagnostics_ToolHelp`.
  - [win_service.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/win_service.rs): `find_winlogon_pid_in_session` + `open_session_system_token` (token de `winlogon.exe` con `SecurityImpersonation`, fallback a Session 0) y `CreateProcessAsUserW` con `DETACHED_PROCESS`.
  - [input.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/input.rs): logger asíncrono en segundo plano sin bloqueo (`C:\ProgramData\RandyDesk\host_input_debug.log` en `SYSTEM`), `desktop_names_match`, `GENERIC_WRITE` + `CloseDesktop(current_desk)`, liberación de `Shift` en `UnicodeChar` y fallback `MAPVK_VK_TO_VSC`.
  - [win_hook.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/win_hook.rs) y [app.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydesk-client/src/app.rs): refresco de `state.tx` vía `same_channel`, `is_altgr_down()` (`VK_RMENU`), reenvío de `Copy`/`Cut`/`Paste` (`send_remote_ctrl_shortcut`) y trazas `[CLIENT-EGUI]` no bloqueantes.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 44 pruebas aprobadas (19 `randydesk-client`, 15 unit + 1 `quic_loopback` `randydesk-host`, 8 `randydesk-proto`, 1 `relay_e2e` `randydesk-relay`).
  - `cargo build --workspace --release`: compilación limpia de `target\release\RandyDesk.exe` (`12,352,512` bytes) y `target\release\randydesk-relay.exe` (`2,768,384` bytes, copiado a `dist\`; `dist\RandyDesk.exe` en uso por `v0.5.0` abierta).

## 2026-11-26 — RandyDesk v0.5.0 (Fase 2): Servicio `SYSTEM` (`SendSAS` / UAC), códec H.264 y promoción directa LAN/Tailscale
- **Resumen técnico:**
  - Versión subida a `0.5.0` en `Cargo.toml` (y `Cargo.lock`), agregando `openh264 = "0.9"` (`openh264 v0.9.10` compilado sin NASM externo vía MSVC) y features Win32 `Win32_Security_Authorization`, `Win32_System_Registry`, `Win32_System_RemoteDesktop`, `Win32_System_Services`.
  - Protocolo `v2` compatible hacia atrás (`MIN_PROTOCOL_VERSION = 1..=2`): agregados `CodecKind::H264`, `ClientMsg::SendSas` y `HostMsg::DirectEndpoints(Vec<String>)`.
  - Servicio Windows `NT AUTHORITY\SYSTEM` ([win_service.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/win_service.rs)): instala/desinstala `RandyDeskService` (`C:\Program Files\RandyDesk\RandyDesk.exe`), migra ID/certificados TLS a `C:\ProgramData\RandyDesk\`, configura `SoftwareSASGeneration = 3`, ejecuta supervisor Session 0 → sesión consola activa (`CreateProcessAsUserW`) y permite `Ctrl+Alt+Del` (`sas.dll!SendSAS`), captura de UAC/pantalla de bloqueo (`capture.rs` cambia de escritorio con `sync_thread_to_input_desktop` y fallback GDI) y gestión con 1 clic en `host_panel.rs`.
  - Códec H.264 ([h264_codec.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-host/src/h264_codec.rs) + [decoder.rs](file:///c:/Users/Flavio/Desktop/H4CK/Flavio/randydeskt/crates/randydesk-client/src/decoder.rs)): streaming inter-frame OpenH264 (`ScreenContentRealTime`) con prevención de descarte de P-frames en `LatestFrameSlot`, más promoción automática de sesión Relay a QUIC directo en LAN/Tailscale (`probe_direct_quic` + `SwitchToDirect`).
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 41 pruebas aprobadas (18 en `randydesk-client`, 13 unit + 1 `quic_loopback` en `randydesk-host`, 8 en `randydesk-proto`, 1 `relay_e2e` en `randydesk-relay`).
  - `cargo build --workspace --release` + copia a `dist\`: `RandyDesk.exe` (`12,302,848` bytes) y `randydesk-relay.exe` (`2,768,384` bytes).
- **Pendiente de verificación en vivo:**
  - Probar instalación del servicio `SYSTEM`, `Ctrl+Alt+Del (SAS)`, UAC/LockScreen, códec `H.264` y promoción automática LAN/Tailscale en ambas PCs.

## 2026-11-26 — RandyDesk v0.4.6 (Fase 1): Fix escritura/`win_down`/AltGr, barra colapsable y downscale fluido
- **Resumen técnico:**
  - Versión subida a `0.4.6` en `Cargo.toml` (y `Cargo.lock`), activando `Win32_System_Shutdown` para `LockWorkStation`.
  - `crates/randydesk-client/src/win_hook.rs`, `tray.rs`, `app.rs` y `crates/randydesk-host/src/input.rs`: aislada la regresión de escritura en `v0.4.5` (`input_debug.log` mostró `win_down` enclavado si `Win` se soltaba sobre `8488`/`7460` y `fg_pid=8480` de `SearchHost.exe` bloqueando `egui::Event::Text`). Se limpia `win_down` incondicionalmente en todo `WM_KEYUP`/`WM_SYSKEYUP` + resincronización con `GetAsyncKeyState`, se añade fallback `should_hook_capture_all_keys` cuando otra app retiene foreground con cursor encima, `SetActiveWindow`/`SetFocus` en `force_foreground`, soporte AltGr (`@`, `\`, `{}`) y log `[HOST-UNI]` ante fallos UIPI.
  - `crates/randydesk-client/src/session_bar.rs` y `app.rs`: barra flotante colapsable en pestaña mínima superior (`Ocultar` + `Fijar`), escala personalizada (`50%..=200%`), `Resolución de transmisión` (`100%`/`75%`/`50%`), `Mostrar cursor remoto`, `Bloquear pantalla remota`, `Ctrl+Alt+Del` marcado para Fase 2 (`SYSTEM`), `Invertir dirección de rueda`, `Intercambiar botones de ratón` y `Solo visualización`.
  - `crates/randydesk-proto/src/message.rs`, `crates/randydesk-host/src/{encoder.rs,power.rs,session.rs,relay_agent.rs}`, `crates/randydesk-client/src/{net.rs,relay_client.rs}`: variantes compatibles `ClientMsg::SetStreamScale(u8)` y `ClientMsg::LockWorkStation`, downscale rápido BGRA en `TileEncoder::encode_scaled` (promedio 2x2 al 50% y punto fijo 16.16), y `yield_now()` bajo saturación de envío en Relay.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 37 pruebas aprobadas (17 en `randydesk-client`, 10 unit + 1 `quic_loopback` en `randydesk-host`, 8 en `randydesk-proto`, 1 `relay_e2e` en `randydesk-relay`).
  - `cargo build --workspace --release` + copia a `dist\`: `RandyDesk.exe` (`11,395,072` bytes) y `randydesk-relay.exe` (`2,766,336` bytes).
- **Pendiente de verificación en vivo y siguiente fase:**
  - Probar `v0.4.6` en ambas PCs y confirmar inicio de **Fase 2 (`v0.5.0`)**: Servicio de Windows `NT AUTHORITY\SYSTEM` + `SendSAS` + códec H.264/P2P directo.

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
