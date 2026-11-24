# RandyDesk

Software ligero de escritorio remoto para Windows escrito en **Rust puro**, inspirado en RustDesk y optimizado para **sesiones ilimitadas (2–24+ horas)** entre tus equipos sin límites comerciales ni servidores pagos.

## Ejecutable unificado (`RandyDesk.exe`)

Al abrir **`dist\RandyDesk.exe`** (o `target\release\RandyDesk.exe`) con doble clic se abre la interfaz gráfica sin consola negra:
- **Panel izquierdo (`Tu Escritorio`):** enciende automáticamente la recepción de conexiones en el puerto UDP `21118`, muestra tu IP LAN y tu IP Tailscale, y genera o permite fijar un PIN/contraseña.
- **Panel derecho (`Controlar escritorio remoto` + `Mis Equipos`):** guarda tus 3–4 PCs para conectarte en 1 clic con cifrado QUIC TLS 1.3, verificación TOFU, modo Fluido (JPEG) o Nítido (LZ4), portapapeles y reconexión automática.

*(Opcional: `target\release\randydesk-host.exe` sigue disponible si quieres correr el agente en modo consola/servicio sin ventana).*

## Compilación

```powershell
cargo build --workspace --release
```

## Uso en red local (LAN) o por Internet (\$0 con Tailscale)

1. Copia **`RandyDesk.exe`** en tus 3–4 computadoras y ábrelo (acepta el acceso UDP `21118` en el Firewall de Windows si te lo pide).
2. Si las PCs están en casas u oficinas distintas, instala **Tailscale** (plan gratuito) en ellas y usa la IP `100.x.y.z:21118` que aparece automáticamente en el panel **Tu Escritorio**.
3. En las PCs a las que te conectas por **2–24+ horas**, configura **Energía y suspensión → Suspender el equipo: Nunca**.

## Verificación técnica

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
