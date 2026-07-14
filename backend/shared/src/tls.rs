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

#[cfg(test)]
mod tests {
    use super::*;

    // -- Tests that require network access --

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn async_client_builds_and_requests_https() {
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build async client");
        let resp = client
            .get("https://example.com")
            .send()
            .await
            .expect("async HTTPS request failed");
        assert!(resp.status().is_success());
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn tls_works_without_system_certs() {
        let orig_cert_dir = std::env::var_os("SSL_CERT_DIR");
        let orig_cert_file = std::env::var_os("SSL_CERT_FILE");
        unsafe {
            std::env::set_var("SSL_CERT_DIR", "/nonexistent");
            std::env::set_var("SSL_CERT_FILE", "/nonexistent");
        }
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build client without system certs");
        let resp = client
            .get("https://example.com")
            .send()
            .await
            .expect("HTTPS request failed without system certs");
        assert!(resp.status().is_success());
        unsafe {
            match orig_cert_dir {
                Some(v) => std::env::set_var("SSL_CERT_DIR", v),
                None => std::env::remove_var("SSL_CERT_DIR"),
            }
            match orig_cert_file {
                Some(v) => std::env::set_var("SSL_CERT_FILE", v),
                None => std::env::remove_var("SSL_CERT_FILE"),
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn invalid_proxy_url_ignored() {
        set_proxy_url(Some("not-a-valid-url".to_string()));
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("builder should succeed even with invalid proxy URL");
        let resp = client.get("https://example.com").send().await;
        assert!(resp.is_ok(), "request should succeed without proxy applied");
        set_proxy_url(None);
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn no_proxy_by_default() {
        set_proxy_url(None);
        assert_eq!(proxy_url(), None);
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("builder should succeed without proxy");
        let resp = client.get("https://example.com").send().await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn insecure_builder_works() {
        let client = client_builder_insecure()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build insecure client");
        let resp = client.get("https://example.com").send().await;
        assert!(resp.is_ok(), "insecure client should be able to make requests");
    }

    // -- Tests that do NOT require network --

    #[test]
    fn proxy_set_and_get() {
        set_proxy_url(Some("http://127.0.0.1:8080".to_string()));
        assert_eq!(proxy_url(), Some("http://127.0.0.1:8080".to_string()));
        set_proxy_url(None);
        assert_eq!(proxy_url(), None);
    }

    #[test]
    fn proxy_switching() {
        set_proxy_url(Some("http://proxy1:8080".to_string()));
        assert_eq!(proxy_url(), Some("http://proxy1:8080".to_string()));
        set_proxy_url(Some("http://proxy2:3128".to_string()));
        assert_eq!(proxy_url(), Some("http://proxy2:3128".to_string()));
        set_proxy_url(None);
        assert_eq!(proxy_url(), None);
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn proxy_applied_to_builder() {
        set_proxy_url(Some("http://127.0.0.1:9999".to_string()));
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("failed to build client with proxy");
        let result = client.get("https://example.com").send().await;
        assert!(result.is_err(), "request through non-existent proxy should fail");
        set_proxy_url(None);
    }

    #[test]
    fn builder_creates_valid_client() {
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();
        assert!(client.is_ok(), "client_builder should produce a valid client");
    }

    #[test]
    fn insecure_builder_creates_valid_client() {
        let client = client_builder_insecure()
            .timeout(std::time::Duration::from_secs(5))
            .build();
        assert!(client.is_ok(), "client_builder_insecure should produce a valid client");
    }

    #[test]
    fn proxy_builder_creates_valid_client() {
        set_proxy_url(Some("http://127.0.0.1:8080".to_string()));
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();
        assert!(client.is_ok(), "client_builder with proxy should produce a valid client");
        set_proxy_url(None);
    }

    #[test]
    fn invalid_proxy_does_not_break_builder() {
        set_proxy_url(Some("not-a-valid-url://??".to_string()));
        let client = client_builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();
        assert!(client.is_ok(), "invalid proxy URL should not prevent client creation");
        set_proxy_url(None);
    }
}
