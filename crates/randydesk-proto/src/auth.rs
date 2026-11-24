//! Primitivas criptográficas de autenticación por reto (challenge-response) y generación de PIN.

use rand::{rngs::OsRng, RngCore};
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
}
