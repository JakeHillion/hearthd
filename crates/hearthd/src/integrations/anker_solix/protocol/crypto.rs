use aes_gcm::Aes128Gcm;
use aes_gcm::KeyInit;
use aes_gcm::Nonce;
use aes_gcm::aead::Aead;
use aes_gcm::aead::Payload;
use anyhow::ensure;
use p256::PublicKey;
use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::rand_core::OsRng;
use p256::elliptic_curve::sec1::ToEncodedPoint;

const AAD: [u8; 16] = [0x33, 0x22, 0x11, 0, 0x77, 0x66, 0x55, 0x44, 0xbb, 0xaa, 0x99, 0x88, 0xff, 0xee, 0xdd, 0xcc];

/// Deliberately has no Debug implementation: key material is never diagnostic.
pub struct Cipher {
    cipher: Aes128Gcm,
    nonce: [u8; 12],
}

impl Cipher {
    pub fn negotiation() -> Self {
        Self::new(&[0xb8, 0xff, 0x74, 0x22, 0x95, 0x5d, 0x4e, 0xb6, 0xd5, 0x54, 0xa2, 0xc4, 0x70, 0x28, 5, 0x59], [0x6b, 0xa3, 0xe3, 0xf2, 0xf3, 0xa6, 0x0f, 0x29, 0x71, 0xce, 0x5d, 0x1f])
    }

    fn new(key: &[u8], nonce: [u8; 12]) -> Self {
        Self { cipher: Aes128Gcm::new_from_slice(key).expect("fixed AES key width"), nonce }
    }

    pub fn from_shared(shared: &[u8; 32]) -> Self {
        Self::new(&shared[..16], shared[16..28].try_into().unwrap())
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.cipher.encrypt(&Nonce::from(self.nonce), Payload { msg: plaintext, aad: &AAD }).map_err(|_| anyhow::anyhow!("AES-GCM encryption failed"))
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        ensure!(ciphertext.len() >= 16, "missing AES-GCM authentication tag");
        self.cipher.decrypt(&Nonce::from(self.nonce), Payload { msg: ciphertext, aad: &AAD }).map_err(|_| anyhow::anyhow!("AES-GCM authentication failed"))
    }
}

pub struct Exchange(EphemeralSecret);

impl Exchange {
    pub fn new() -> Self { Self(EphemeralSecret::random(&mut OsRng)) }
    pub fn public(&self) -> Vec<u8> { self.0.public_key().to_encoded_point(false).as_bytes()[1..].to_vec() }
    pub fn finish(self, point: &[u8]) -> anyhow::Result<Cipher> {
        ensure!(point.len() == 64, "invalid P-256 public key width");
        let mut encoded = vec![4];
        encoded.extend_from_slice(point);
        let key = PublicKey::from_sec1_bytes(&encoded).map_err(|_| anyhow::anyhow!("invalid P-256 public point"))?;
        let shared = self.0.diffie_hellman(&key);
        Ok(Cipher::from_shared(shared.raw_secret_bytes().as_ref()))
    }
}
