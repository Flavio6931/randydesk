# RandyDesk

Software ligero de escritorio remoto para Windows escrito en **Rust puro**, inspirado en RustDesk y optimizado para **sesiones ilimitadas (2–24+ horas)** sin depender de Tailscale ni servicios pagos.

## Cómo conectarse sin Tailscale (ID numérico + Relay E2E)

1. **Tu servidor Relay (`randydesk-relay`):**
   - **Para probar hoy en 10 segundos** desde tu PC (si tienes `cloudflared`):
     ```powershell
     .\dist\randydesk-relay.exe
     cloudflared tunnel --url http://127.0.0.1:21119
     ```
     Copia la URL `https://xxxx.trycloudflare.com` que imprime `cloudflared`.
   - **Para dejarlo gratis 24/7 en la nube:** conecta este repositorio en **Render** (incluye `Dockerfile` y `render.yaml` con `plan: free`).
2. **En ambas PCs (la tuya y la de tu compañero):**
   - Abran **`RandyDesk.exe`**, pulsen **`Configurar servidor Relay`** en la barra inferior y peguen la URL del Relay.
   - El panel izquierdo mostrará **`● Listo en Internet (Relay)`** debajo de tu **ID de 9 dígitos** (ej. `482 910 331`).
   - Escribe el ID de tu compañero, pon su contraseña/PIN y listo: todo el video, teclado y ratón viajan cifrados **E2E (X25519 + ChaCha20-Poly1305)**.

*(En red local Wi-Fi/oficina funciona directo sin Relay escribiendo la IP `192.168.x.x`).*

## Compilación y verificación

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```
