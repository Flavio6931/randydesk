# Registro de Sesiones — RandyDesk

## 2026-11-24 — RandyDesk v0.2.0: Rediseño visual estilo RustDesk y Release `v0.2.0`
- **Regla incorporada:** Cada mejora sube la versión en `Cargo.toml` y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **Resumen técnico:**
  - Incrementada versión del workspace a `0.2.0`.
  - Creados `crates/randydesk-client/src/theme.rs` (paleta oscura estilo RustDesk y logotipo vectorial Windows sin emojis rotos) y `crates/randydesk-client/src/cards.rs` (pestañas Recientes / Favoritos / Todos, buscador, tarjetas de colores con menú `...`).
  - Simplificado `host_panel.rs` (bloques con barra de acento azul/verde, copia con 1 clic, opciones técnicas ocultas en menú `...`) y `app.rs` (tarjeta superior "Controlar escritorio remoto", modales de contraseña, editor de equipo y guía rápida, barra inferior de estado).
  - Extendido `config.rs` de forma retrocompatible con `#[serde(default)]` (`favorite`, `last_connected`, `color_idx`), `display_address` y `upsert_recent`.
- **Verificación ejecutada:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 errores, 0 warnings.
  - `cargo test --workspace`: 14 pruebas aprobadas (5 en `randydesk-client`, 3 unitarias en `randydesk-host`, 1 integración `quic_loopback`, 5 en `randydesk-proto`).
  - `cargo build -p randydesk-client --release`: generado `target\release\RandyDesk.exe` (`9,692,160` bytes). *(Nota: `dist\RandyDesk.exe` estaba abierto en uso y no se sobreescribió localmente).*
  - Publicado Release `v0.2.0` en `Flavio6931/randydesk` adjuntando `target\release\RandyDesk.exe`.
- **Pendiente de verificación en vivo:**
  - Cerrar la ventana abierta de `dist\RandyDesk.exe` (v0.1.0), reemplazarlo con `target\release\RandyDesk.exe` (v0.2.0) y probar en vivo con tu compañero.
