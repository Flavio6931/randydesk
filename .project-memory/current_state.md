# Estado Actual — RandyDesk

- **Proyecto:** RandyDesk (escritorio remoto en Rust puro para Windows, inspirado en RustDesk)
- **Versión actual:** `0.3.0`
- **Regla del proyecto:** Cada mejora incrementa la versión en `Cargo.toml` (`[workspace.package].version`) y publica un nuevo Release `vX.Y.Z` en GitHub con `RandyDesk.exe`.
- **GitHub vinculado:** cuenta `Flavio6931` (`Flavio Morales <copiaste2024@gmail.com>`)
- **Repositorio:** https://github.com/Flavio6931/randydesk
- **Último Release:** `v0.3.0` — https://github.com/Flavio6931/randydesk/releases/tag/v0.3.0
- **Descarga directa (`RandyDesk.exe`):** https://github.com/Flavio6931/randydesk/releases/download/v0.3.0/RandyDesk.exe
- **Bóveda Obsidian:** `RandyDesk Vault` (`C:\Users\Flavio\Documents\RandyDesk Vault`)

## Tareas Pendientes
- [ ] Prueba visual en vivo con tu compañero usando `RandyDesk v0.3.0` + `randydesk-relay.exe` + `cloudflared` (o Render Free con `Dockerfile`/`render.yaml`)
- [ ] Opcional: preconfigurar URL fija de Render dentro de `RandyDesk.exe` una vez desplegado el servicio web gratuito

## Últimas Tareas Completadas
- Implementación de `randydesk-proto/src/e2e.rs` (X25519 + ChaCha20-Poly1305 con nonces monotónicos) y generador/parser de IDs de 9 dígitos en `auth.rs`.
- Creación del servidor zero-knowledge `crates/randydesk-relay` (`/ws/host`, `/ws/client`, `/ws/bridge`, `/health`) + `Dockerfile` y `render.yaml` (`plan: free`).
- Integración del modo dual en `randydesk-host` (`relay_agent.rs`, `service.rs`) y `randydesk-client` (`relay_client.rs`, `net.rs`, `settings.json`), más fixes visuales de PIN completo, barra de sesión sin solapamientos y modal "Configurar servidor Relay".
- Verificación completa: `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings), `cargo test --workspace` (18/18 tests incluyendo `relay_e2e`), `cargo build --workspace --release` (`dist\RandyDesk.exe` `10,789,376` bytes y `dist\randydesk-relay.exe` `2,765,824` bytes) y publicación en GitHub Release `v0.3.0`.
