use once_cell::sync::Lazy;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::SignatureScheme;
use std::sync::Arc;

fn base_config_builder() -> rustls::ConfigBuilder<rustls::ClientConfig, rustls::WantsVerifier> {
    static PROVIDER: Lazy<Arc<rustls::crypto::CryptoProvider>> =
        Lazy::new(|| Arc::new(rustls::crypto::ring::default_provider()));
    rustls::ClientConfig::builder_with_provider(PROVIDER.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports all safe default protocol versions")
}

pub fn client_builder() -> reqwest::ClientBuilder {
    static CONFIG: Lazy<rustls::ClientConfig> = Lazy::new(|| {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.roots = webpki_roots::TLS_SERVER_ROOTS.to_vec();
        base_config_builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    });
    reqwest::Client::builder().use_preconfigured_tls(CONFIG.clone())
}

pub fn client_builder_insecure() -> reqwest::ClientBuilder {
    static CONFIG: Lazy<rustls::ClientConfig> = Lazy::new(|| {
        base_config_builder()
            .dangerous()
            .with_custom_certificate_verifier(VERIFIER.clone())
            .with_no_client_auth()
    });
    reqwest::Client::builder().use_preconfigured_tls(CONFIG.clone())
}

static VERIFIER: Lazy<Arc<AcceptAllVerifier>> = Lazy::new(|| {
    let provider = rustls::crypto::ring::default_provider();
    Arc::new(AcceptAllVerifier(provider.signature_verification_algorithms))
});

#[derive(Debug)]
struct AcceptAllVerifier(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAllVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}
