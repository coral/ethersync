use crate::transport::Error;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn validate_pin(pin: &str) -> Result<(), Error> {
    if pin.len() != 64 || !pin.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Invalid(
            "certificate pin must contain 64 hexadecimal digits",
        ));
    }
    Ok(())
}
fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}
#[derive(Debug)]
struct Verify {
    pin: Option<String>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}
impl ServerCertVerifier for Verify {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if self
            .pin
            .as_ref()
            .is_some_and(|pin| !pin.eq_ignore_ascii_case(&fingerprint(cert)))
        {
            return Err(rustls::Error::General(
                "certificate fingerprint mismatch".into(),
            ));
        }
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
pub fn client(pin: Option<&str>) -> Result<quinn_proto::ClientConfig, Error> {
    if let Some(pin) = pin {
        validate_pin(pin)?;
    }
    let provider = provider();
    let mut tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(Error::quic)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Verify {
            pin: pin.map(String::from),
            provider,
        }))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"moq-lite-05".to_vec()];
    let mut config = quinn_proto::ClientConfig::new(Arc::new(
        quinn_proto::crypto::rustls::QuicClientConfig::try_from(tls).map_err(Error::quic)?,
    ));
    config.transport_config(Arc::new(transport()));
    Ok(config)
}
pub fn server() -> Result<(quinn_proto::ServerConfig, String), Error> {
    let mut params =
        rcgen::CertificateParams::new(vec!["localhost".into()]).map_err(Error::quic)?;
    params.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(1);
    params.not_after = params.not_before + time::Duration::days(13);
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(Error::quic)?;
    let cert = params.self_signed(&key).map_err(Error::quic)?;
    let fp = fingerprint(cert.der());
    let mut tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(Error::quic)?
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .map_err(Error::quic)?;
    tls.alpn_protocols = vec![b"moq-lite-05".to_vec(), b"h3".to_vec()];
    let mut config = quinn_proto::ServerConfig::with_crypto(Arc::new(
        quinn_proto::crypto::rustls::QuicServerConfig::try_from(tls).map_err(Error::quic)?,
    ));
    config.transport_config(Arc::new(transport()));
    Ok((config, fp))
}
fn transport() -> quinn_proto::TransportConfig {
    let mut t = quinn_proto::TransportConfig::default();
    t.keep_alive_interval(Some(std::time::Duration::from_secs(1)));
    t.max_idle_timeout(Some(std::time::Duration::from_secs(3).try_into().unwrap()));
    t.max_concurrent_uni_streams(64u32.into());
    t.max_concurrent_bidi_streams(16u32.into());
    t.stream_receive_window(65536u32.into());
    t.receive_window(1048576u32.into());
    t.datagram_receive_buffer_size(Some(65536));
    t.datagram_send_buffer_size(65536);
    t
}
