use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct TlsCertificate {
    pub cert_pem: String,
    pub key_pem: String,
    pub fingerprint: String,
}

impl TlsCertificate {
    pub fn generate() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let subject_alt_names = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];
        let params = rcgen::CertificateParams::new(subject_alt_names)?;
        let key_pair = rcgen::KeyPair::generate()?;
        let cert = params.self_signed(&key_pair)?;
        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();

        let der = cert.der();
        let mut hasher = Sha256::new();
        hasher.update(der.as_ref());
        let digest = hasher.finalize();
        let fingerprint = hex::encode(digest).to_uppercase();

        Ok(Self {
            cert_pem,
            key_pem,
            fingerprint,
        })
    }
}
