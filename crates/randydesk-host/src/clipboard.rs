//! Acceso seguro al portapapeles de texto del host sin propagar fallos fatales.

use arboard::Clipboard;
use tracing::warn;

/// Lee el texto actual del portapapeles del sistema, o devuelve cadena vacía si no hay texto.
pub fn get_text() -> String {
    match Clipboard::new().and_then(|mut cb| cb.get_text()) {
        Ok(text) => text,
        Err(err) => {
            warn!("No se pudo leer portapapeles del host: {err}");
            String::new()
        }
    }
}

/// Escribe texto UTF-8 en el portapapeles del sistema.
pub fn set_text(text: &str) {
    if let Err(err) = Clipboard::new().and_then(|mut cb| cb.set_text(text.to_owned())) {
        warn!("No se pudo escribir en portapapeles del host: {err}");
    }
}
