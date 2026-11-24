//! Primitivas de autenticación por reto, PIN temporal e IDs numéricos de 9 dígitos.

use rand::{rngs::OsRng, Rng, RngCore};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Separador de dominio para derivar la prueba criptográfica sin exponer la contraseña.
const AUTH_DOMAIN: &[u8] = b"randydesk-auth-v1";

/// Alfabeto legible para PIN de sesión (excluye 0/O/1/I para evitar confusiones visuales).
const PIN_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// Genera un nonce aleatorio de 256 bits usando el CSPRNG del sistema operativo.
pub fn generate_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Calcula la prueba SHA-256 combinando el separador de dominio, el `nonce` y la contraseña.
pub fn compute_proof(nonce: &[u8; 32], password: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(AUTH_DOMAIN);
    hasher.update(nonce);
    hasher.update(password.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Verifica en tiempo constante que `received` coincida con la prueba esperada para `password`.
pub fn verify_proof(nonce: &[u8; 32], password: &str, received: &[u8; 32]) -> bool {
    let expected = compute_proof(nonce, password);
    expected.ct_eq(received).into()
}

/// Genera un PIN temporal de 8 caracteres alfanuméricos sin ambigüedad visual.
pub fn generate_session_pin() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| PIN_ALPHABET[(*b as usize) % PIN_ALPHABET.len()] as char)
        .collect()
}

/// Genera un ID numérico aleatorio de 9 dígitos estilo RustDesk (`100000000`..=`999999999`).
pub fn generate_host_id() -> String {
    let num: u32 = OsRng.gen_range(100_000_000..=999_999_999);
    num.to_string()
}

/// Formatea un ID de 9 dígitos con espacios cada 3 dígitos (`"482910331"` -> `"482 910 331"`).
pub fn format_host_id(id: &str) -> String {
    let digits: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 9 {
        format!("{} {} {}", &digits[0..3], &digits[3..6], &digits[6..9])
    } else {
        id.to_string()
    }
}

/// Extrae un ID numérico exacto de 9 dígitos ignorando espacios y guiones; devuelve `None` si es IP/hostname.
pub fn parse_numeric_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.contains('.') || trimmed.contains(':') {
        return None;
    }
    let digits: String = trimmed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if digits.len() == 9 && digits.chars().all(|c| c.is_ascii_digit()) {
        Some(digits)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_challenge_accepts_valid_and_rejects_invalid() {
        let nonce = generate_nonce();
        let proof = compute_proof(&nonce, "mi_clave_segura");
        assert!(verify_proof(&nonce, "mi_clave_segura", &proof));
        assert!(!verify_proof(&nonce, "otra_clave", &proof));

        let mut tampered = proof;
        tampered[0] ^= 0xFF;
        assert!(!verify_proof(&nonce, "mi_clave_segura", &tampered));
    }

    #[test]
    fn session_pin_has_expected_length_and_charset() {
        let pin = generate_session_pin();
        assert_eq!(pin.len(), 8);
        assert!(pin.chars().all(|c| PIN_ALPHABET.contains(&(c as u8))));
    }

    #[test]
    fn host_id_generation_formatting_and_parsing() {
        let id = generate_host_id();
        assert_eq!(id.len(), 9);
        assert!(id.chars().all(|c| c.is_ascii_digit()));

        let formatted = format_host_id("482910331");
        assert_eq!(formatted, "482 910 331");

        assert_eq!(parse_numeric_id("482 910 331").as_deref(), Some("482910331"));
        assert_eq!(parse_numeric_id("482-910-331").as_deref(), Some("482910331"));
        assert_eq!(parse_numeric_id("192.168.1.10"), None);
        assert_eq!(parse_numeric_id("12345"), None);
    }
}
