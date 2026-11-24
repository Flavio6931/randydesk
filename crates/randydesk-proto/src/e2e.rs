//! Cifrado extremo a extremo (X25519 + ChaCha20-Poly1305) para túneles sobre servidor Relay.

use crate::MAX_FRAME_BYTES;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::rngs::OsRng;
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{EphemeralSecret, PublicKey};

/// Errores posibles en el cifrado o descifrado E2E.
#[derive(Debug, Error)]
pub enum E2eError {
    /// Error de serialización binaria.
    #[error("error serializando payload E2E: {0}")]
    Codec(#[from] bincode::Error),
    /// Tamaño de paquete excede `MAX_FRAME_BYTES`.
    #[error("paquete E2E excede el máximo permitido")]
    PayloadTooLarge,
    /// Fallo criptográfico (clave incorrecta, paquete alterado o fuera de orden).
    #[error("fallo de autenticación o descifrado ChaCha20-Poly1305")]
    CryptoFailure,
}

/// Par de claves efímero X25519 para un handshake de sesión único.
pub struct E2eKeypair;

impl E2eKeypair {
    /// Genera una clave secreta efímera y su clave pública de 32 bytes.
    pub fn generate() -> (EphemeralSecret, [u8; 32]) {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret).to_bytes();
        (secret, public)
    }
}

/// Mitad emisora del canal cifrado E2E para uso concurrente en tareas separadas.
pub struct E2eEncryptor {
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl E2eEncryptor {
    /// Serializa y cifra un mensaje usando el contador monotónico de envío.
    pub fn encrypt_msg<T: Serialize>(&mut self, msg: &T) -> Result<Vec<u8>, E2eError> {
        let plain = bincode::serialize(msg)?;
        if plain.len() > MAX_FRAME_BYTES {
            return Err(E2eError::PayloadTooLarge);
        }
        let nonce = counter_to_nonce(self.counter);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plain.as_slice())
            .map_err(|_| E2eError::CryptoFailure)?;
        self.counter = self.counter.checked_add(1).ok_or(E2eError::CryptoFailure)?;
        Ok(ciphertext)
    }
}

/// Mitad receptora del canal cifrado E2E para uso concurrente en tareas separadas.
pub struct E2eDecryptor {
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl E2eDecryptor {
    /// Descifra, autentica y deserializa un mensaje verificando el contador esperado.
    pub fn decrypt_msg<T: DeserializeOwned>(&mut self, data: &[u8]) -> Result<T, E2eError> {
        if data.len() > MAX_FRAME_BYTES + 64 {
            return Err(E2eError::PayloadTooLarge);
        }
        let nonce = counter_to_nonce(self.counter);
        let plain = self
            .cipher
            .decrypt(&nonce, data)
            .map_err(|_| E2eError::CryptoFailure)?;
        self.counter = self.counter.checked_add(1).ok_or(E2eError::CryptoFailure)?;
        let msg = bincode::deserialize(&plain)?;
        Ok(msg)
    }
}

/// Canal cifrado bidireccional con claves direccionales y contadores anti-replay monotónicos.
pub struct E2eCipher {
    enc: E2eEncryptor,
    dec: E2eDecryptor,
}

impl E2eCipher {
    /// Deriva claves direccionales `c2h` y `h2c` a partir del secreto compartido X25519.
    pub fn from_handshake(
        secret: EphemeralSecret,
        peer_public_bytes: [u8; 32],
        is_client: bool,
    ) -> Self {
        let peer_pub = PublicKey::from(peer_public_bytes);
        let shared = secret.diffie_hellman(&peer_pub);

        let c2h_key = derive_directional_key(shared.as_bytes(), b"randydesk-e2e-c2h-v1");
        let h2c_key = derive_directional_key(shared.as_bytes(), b"randydesk-e2e-h2c-v1");

        let (send_key, recv_key) = if is_client {
            (c2h_key, h2c_key)
        } else {
            (h2c_key, c2h_key)
        };

        Self {
            enc: E2eEncryptor {
                cipher: ChaCha20Poly1305::new(Key::from_slice(&send_key)),
                counter: 0,
            },
            dec: E2eDecryptor {
                cipher: ChaCha20Poly1305::new(Key::from_slice(&recv_key)),
                counter: 0,
            },
        }
    }

    /// Divide el canal cifrado en emisor y receptor independientes para escritura/lectura paralela.
    pub fn split(self) -> (E2eEncryptor, E2eDecryptor) {
        (self.enc, self.dec)
    }

    /// Serializa y cifra un mensaje usando el contador monotónico de envío.
    pub fn encrypt_msg<T: Serialize>(&mut self, msg: &T) -> Result<Vec<u8>, E2eError> {
        self.enc.encrypt_msg(msg)
    }

    /// Descifra, autentica y deserializa un mensaje verificando el contador esperado.
    pub fn decrypt_msg<T: DeserializeOwned>(&mut self, data: &[u8]) -> Result<T, E2eError> {
        self.dec.decrypt_msg(data)
    }
}

fn derive_directional_key(shared_secret: &[u8; 32], domain: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(shared_secret);
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn counter_to_nonce(counter: u64) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[4..12].copy_from_slice(&counter.to_le_bytes());
    *Nonce::from_slice(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMsg, HostMsg, PROTOCOL_VERSION};

    #[test]
    fn e2e_handshake_bidirectional_and_tamper_rejection() {
        let (c_sec, c_pub) = E2eKeypair::generate();
        let (h_sec, h_pub) = E2eKeypair::generate();

        let mut client = E2eCipher::from_handshake(c_sec, h_pub, true);
        let mut host = E2eCipher::from_handshake(h_sec, c_pub, false);

        let hello = ClientMsg::Hello {
            version: PROTOCOL_VERSION,
        };
        let enc_hello = client.encrypt_msg(&hello).expect("cifrar cliente");
        let dec_hello: ClientMsg = host.decrypt_msg(&enc_hello).expect("descifrar host");
        assert_eq!(hello, dec_hello);

        let pong = HostMsg::Pong(987);
        let enc_pong = host.encrypt_msg(&pong).expect("cifrar host");
        let dec_pong: HostMsg = client.decrypt_msg(&enc_pong).expect("descifrar cliente");
        assert_eq!(pong, dec_pong);

        // Paquete repetido (replay) debe fallar por nonce desfasado
        assert!(host.decrypt_msg::<ClientMsg>(&enc_hello).is_err());

        // Paquete manipulado debe fallar autenticación Poly1305
        let mut enc_ping = client.encrypt_msg(&ClientMsg::Ping(1)).expect("cifrar ping");
        enc_ping[0] ^= 0x5A;
        assert!(host.decrypt_msg::<ClientMsg>(&enc_ping).is_err());
    }

    #[test]
    fn split_encryptor_and_decryptor_work_concurrently() {
        let (c_sec, c_pub) = E2eKeypair::generate();
        let (h_sec, h_pub) = E2eKeypair::generate();

        let (mut c_enc, mut c_dec) = E2eCipher::from_handshake(c_sec, h_pub, true).split();
        let (mut h_enc, mut h_dec) = E2eCipher::from_handshake(h_sec, c_pub, false).split();

        let enc_ping = c_enc.encrypt_msg(&ClientMsg::Ping(55)).expect("c_enc");
        let dec_ping: ClientMsg = h_dec.decrypt_msg(&enc_ping).expect("h_dec");
        assert_eq!(dec_ping, ClientMsg::Ping(55));

        let enc_pong = h_enc.encrypt_msg(&HostMsg::Pong(55)).expect("h_enc");
        let dec_pong: HostMsg = c_dec.decrypt_msg(&enc_pong).expect("c_dec");
        assert_eq!(dec_pong, HostMsg::Pong(55));
    }
}
