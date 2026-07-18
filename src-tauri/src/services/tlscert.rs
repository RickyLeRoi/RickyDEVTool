use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio_rustls::rustls::{self, pki_types::ServerName};

/// Scadenza del certificato TLS di un host. Handshake con verifier
/// permissivo: la chain viene catturata anche se il certificato è già
/// scaduto o self-signed (è proprio il caso che vogliamo diagnosticare).
/// La validità vera resta compito del check HTTP (reqwest verifica normale).

/// Cache per host: il certificato non cambia di minuto in minuto,
/// mentre il check dei servizi gira ogni 15s.
const CACHE_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug)]
struct AcceptAnyCert(rustls::crypto::CryptoProvider);

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_config() -> &'static Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let provider = rustls::crypto::ring::default_provider();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider.clone()))
            .with_safe_default_protocol_versions()
            .expect("versioni TLS")
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
            .with_no_client_auth();
        Arc::new(config)
    })
}

fn cache() -> &'static Mutex<HashMap<String, (Instant, Option<u64>)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Option<u64>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Scadenza (ms epoch) del certificato di `host:port`, con cache di 1h.
/// None se l'host non parla TLS o l'handshake fallisce.
pub async fn cert_expiry_ms(host: &str, port: u16) -> Option<u64> {
    let key = format!("{host}:{port}");
    if let Some((at, value)) = cache().lock().expect("cert cache").get(&key) {
        if at.elapsed() < CACHE_TTL {
            return *value;
        }
    }
    let result = probe(host, port).await;
    cache()
        .lock()
        .expect("cert cache")
        .insert(key, (Instant::now(), result));
    result
}

async fn probe(host: &str, port: u16) -> Option<u64> {
    let server_name = ServerName::try_from(host.to_string()).ok()?;
    let connect = async {
        let tcp = tokio::net::TcpStream::connect((host, port)).await.ok()?;
        let connector = tokio_rustls::TlsConnector::from(tls_config().clone());
        let tls = connector.connect(server_name, tcp).await.ok()?;
        let (_, session) = tls.get_ref();
        let cert = session.peer_certificates()?.first()?;
        parse_not_after_ms(cert.as_ref())
    };
    tokio::time::timeout(Duration::from_secs(6), connect).await.ok()?
}

pub fn parse_not_after_ms(der: &[u8]) -> Option<u64> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).ok()?;
    let timestamp = cert.validity().not_after.timestamp();
    (timestamp > 0).then(|| timestamp as u64 * 1000)
}

pub fn days_left(expires_at_ms: u64) -> i64 {
    let now = crate::events::now_ms() as i64;
    (expires_at_ms as i64 - now) / 86_400_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_left_calcolo() {
        let now = crate::events::now_ms();
        assert_eq!(days_left(now + 5 * 86_400_000 + 3600_000), 5);
        // Scaduto ieri: negativo.
        assert!(days_left(now - 86_400_000 * 2) < 0);
    }

    // Test di rete (richiede internet): verifica il probe reale su un host noto.
    // `cargo test -- --ignored` per eseguirlo.
    #[tokio::test]
    #[ignore]
    async fn probe_su_host_reale() {
        let expiry = cert_expiry_ms("www.google.com", 443).await;
        assert!(expiry.is_some());
        assert!(days_left(expiry.unwrap()) > 0);
    }
}
