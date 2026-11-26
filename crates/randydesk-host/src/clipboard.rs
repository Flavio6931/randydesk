//! Acceso seguro al portapapeles de texto del host y vigilancia sin eco para sincronización automática.

use arboard::Clipboard;
use tracing::warn;

const DEFAULT_MAX_CLIPBOARD_BYTES: usize = 256 * 1024;

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

/// Devuelve el número de secuencia nativo del portapapeles Win32 sin abrirlo ni bloquearlo.
pub fn sequence_number() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

/// Calcula un hash FNV-1a de 64 bits determinista para detectar cambios reales y evitar bucles eco.
pub fn hash_text(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Vigila el portapapeles de texto local evitando reenviar contenido antiguo o ecos recién aplicados.
pub struct ClipboardWatcher {
    last_seq: u32,
    last_hash: u64,
    max_bytes: usize,
}

impl Default for ClipboardWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardWatcher {
    /// Inicializa el vigilante sincronizado con el contenido actual para no emitir contenido previo.
    pub fn new() -> Self {
        let mut w = Self {
            last_seq: 0,
            last_hash: 0,
            max_bytes: DEFAULT_MAX_CLIPBOARD_BYTES,
        };
        w.reset();
        w
    }

    /// Re-sincroniza el número de secuencia y hash con el estado presente del portapapeles.
    pub fn reset(&mut self) {
        self.last_seq = sequence_number();
        self.last_hash = hash_text(&get_text());
    }

    /// Marca un texto recibido desde el extremo remoto para impedir eco al volver a sondear.
    pub fn mark_applied(&mut self, text: &str) {
        self.last_hash = hash_text(text);
        self.last_seq = sequence_number();
    }

    /// Sondea si cambió el portapapeles local; devuelve `Some(texto)` solo ante cambios nuevos válidos.
    pub fn poll_changed(&mut self) -> Option<String> {
        let seq = sequence_number();
        if seq == self.last_seq {
            return None;
        }
        self.last_seq = seq;
        let text = get_text();
        if self.should_emit_text(&text) {
            Some(text)
        } else {
            None
        }
    }

    pub(crate) fn should_emit_text(&mut self, text: &str) -> bool {
        if text.is_empty() || text.len() > self.max_bytes {
            return false;
        }
        let h = hash_text(text);
        if h == self.last_hash {
            return false;
        }
        self.last_hash = h;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_blocks_echo_duplicates_and_oversized_text() {
        let mut w = ClipboardWatcher {
            last_seq: 1,
            last_hash: hash_text("inicial"),
            max_bytes: 32,
        };

        assert!(!w.should_emit_text(""));
        assert!(!w.should_emit_text("inicial"));
        assert!(w.should_emit_text("hola mundo"));
        assert!(!w.should_emit_text("hola mundo"));

        w.mark_applied("remoto");
        assert!(!w.should_emit_text("remoto"));

        let oversized = "x".repeat(33);
        assert!(!w.should_emit_text(&oversized));
    }
}
