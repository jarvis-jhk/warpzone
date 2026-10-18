//! Outgoing HTTP (GitHub releases, notifications, fault and feedback reports).

/// Agent configuration for every HTTP client the server makes: TLS uses the crypto
/// provider the build chose (feature `ring`, the default, or
/// `pure-rust-crypto`, which wins when both are on).
pub fn http_config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    let config = ureq::Agent::config_builder();
    #[cfg(feature = "pure-rust-crypto")]
    let config =
        config.tls_config(ureq::tls::TlsConfig::builder().unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls_rustcrypto::provider())).build());
    config
}

#[cfg(not(any(feature = "ring", feature = "pure-rust-crypto")))]
compile_error!("warpzone-server needs a TLS crypto provider: feature `ring` (default) or `pure-rust-crypto`");
