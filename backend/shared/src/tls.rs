use log::warn;
use once_cell::sync::Lazy;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::SignatureScheme;
use std::sync::{Arc, RwLock};

/// Global proxy URL configuration. When set, all reqwest clients created
/// through [`client_builder`] will route traffic through this proxy.
static PROXY_URL: RwLock<Option<String>> = RwLock::new(None);

/// Set the global HTTP proxy URL. Pass `None` to disable the proxy.
///
/// This is typically called once at startup from the persisted settings,
/// and again whenever the user updates the proxy setting at runtime.
pub fn set_proxy_url(url: Option<String>) {
    match PROXY_URL.write() {
        Ok(mut guard) => *guard = url,
        Err(e) => warn!("PROXY_URL lock poisoned, cannot update proxy setting: {e}"),
    }
}

/// Read the currently configured proxy URL, if any.
pub fn proxy_url() -> Option<String> {
    match PROXY_URL.read() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            warn!("PROXY_URL lock poisoned, returning None: {e}");
            None
        }
    }
}

fn apply_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    let url = match PROXY_URL.read() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            warn!("PROXY_URL lock poisoned, skipping proxy configuration: {e}");
            return builder;
        }
    };
    match url.as_ref() {
        Some(url) => match reqwest::Proxy::all(url) {
            Ok(proxy) => builder.proxy(proxy),
            Err(e) => {
                warn!("invalid proxy URL: {e}");
                builder
            }
        },
        None => builder,
    }
}

fn base_tls_config() -> rustls::ClientConfig {
    static CONFIG: Lazy<rustls::ClientConfig> = Lazy::new(|| {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.roots = webpki_roots::TLS_SERVER_ROOTS.to_vec();
        base_config_builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    });
    CONFIG.clone()
}

fn apply_blocking_proxy(builder: reqwest::blocking::ClientBuilder) -> reqwest::blocking::ClientBuilder {
    let url = match PROXY_URL.read() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            warn!("PROXY_URL lock poisoned, skipping proxy configuration: {e}");
            return builder;
        }
    };
    match url.as_ref() {
        Some(url) => match reqwest::Proxy::all(url) {
            Ok(proxy) => builder.proxy(proxy),
            Err(e) => {
                warn!("invalid proxy URL: {e}");
                builder
            }
        },
        None => builder,
    }
}

fn base_config_builder() -> rustls::ConfigBuilder<rustls::ClientConfig, rustls::WantsVerifier> {
    static PROVIDER: Lazy<Arc<rustls::crypto::CryptoProvider>> =
        Lazy::new(|| Arc::new(rustls::crypto::ring::default_provider()));
    rustls::ClientConfig::builder_with_provider(PROVIDER.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports all safe default protocol versions")
}

/// Creates a reqwest ClientBuilder configured with the standard WebPKI root trust store
/// and the currently configured global proxy.
pub fn client_builder() -> reqwest::ClientBuilder {
    apply_proxy(reqwest::Client::builder().use_preconfigured_tls(base_tls_config()))
}

/// Creates a reqwest ClientBuilder that disables certificate validation.
///
/// # Warning
/// This builder accepts all certificates without verification and should **only** be used
/// for testing or non-production scenarios where certificate validation is not required.
/// Using this in production bypasses critical security checks.
pub fn client_builder_insecure() -> reqwest::ClientBuilder {
    static CONFIG: Lazy<rustls::ClientConfig> = Lazy::new(|| {
        base_config_builder()
            .dangerous()
            .with_custom_certificate_verifier(VERIFIER.clone())
            .with_no_client_auth()
    });
    apply_proxy(reqwest::Client::builder().use_preconfigured_tls(CONFIG.clone()))
}

/// Creates a blocking reqwest ClientBuilder configured with the standard WebPKI root trust store
/// and the currently configured global proxy.
pub fn blocking_client_builder() -> reqwest::blocking::ClientBuilder {
    apply_blocking_proxy(
        reqwest::blocking::Client::builder().use_preconfigured_tls(base_tls_config()),
    )
}

/// Test whether a given proxy URL is reachable by making a lightweight HTTP request
/// through it. Returns `Ok(())` on success, or an error describing what went wrong.
///
/// Uses a clean client without the global proxy to ensure the candidate proxy
/// is tested in isolation.
pub async fn test_proxy(proxy_url: &str) -> anyhow::Result<()> {
    let proxy = reqwest::Proxy::all(proxy_url)?;
    let client = reqwest::Client::builder()
        .use_preconfigured_tls(base_tls_config())
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client.get("https://example.com").send().await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("proxy returned HTTP {}", resp.status());
    }
}

static VERIFIER: Lazy<Arc<AcceptAllVerifier>> = Lazy::new(|| {
    let provider = rustls::crypto::ring::default_provider();
    Arc::new(AcceptAllVerifier(
        provider.signature_verification_algorithms,
    ))
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
