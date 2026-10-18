//! HMAC-SHA256 webhook signatures in the form the Personas webhook server verifies
//! (`personas/src-tauri/src/engine/webhook.rs`, `verify_hmac_sha256`): the header
//! `x-signature-256: sha256=<lowercase hex of HMAC-SHA256(secret, raw body)>`. Personas
//! fails closed: an unsigned call, or a trigger without a secret, is refused.

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// The header Personas reads (it also accepts `x-hub-signature-256` and
/// `x-webhook-signature`).
pub const SIGNATURE_HEADER: &str = "x-signature-256";

/// A webhook secret. Its `Debug` never shows the value, so it cannot reach a log by accident.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Read the secret from the environment variable `var`. The error names the variable
    /// only.
    pub fn from_env(var: &str) -> Result<Self, String> {
        match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => Ok(Self(v)),
            Ok(_) => Err(format!("environment variable {var} is empty")),
            Err(_) => Err(format!("environment variable {var} is not set")),
        }
    }

    fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    s
}

/// `sha256=<hex>` over the exact bytes that are sent.
pub fn sign(secret: &Secret, body: &[u8]) -> String {
    // HMAC accepts a key of any length, so this cannot fail.
    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.bytes()) {
        Ok(m) => m,
        Err(_) => unreachable!("HMAC takes keys of any length"),
    };
    mac.update(body);
    format!("sha256={}", to_hex(&mac.finalize().into_bytes()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn from_hex(s: &str) -> Result<Vec<u8>, ()> {
        if !s.len().is_multiple_of(2) {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(s.get(i..i + 2).ok_or(())?, 16).map_err(|_| ()))
            .collect()
    }

    /// Ported from Personas `engine/webhook.rs::verify_hmac_sha256`, line for line: strip
    /// an optional `sha256=` prefix, hex-decode (a dummy on failure, so both paths compare
    /// in constant time), verify the MAC, then AND with hex validity.
    pub(crate) fn personas_verify(secret: &str, body: &[u8], signature: &str) -> bool {
        let hex_sig = signature.strip_prefix("sha256=").unwrap_or(signature);
        let dummy = [0u8; 32];
        let (expected_bytes, hex_valid) = match from_hex(hex_sig) {
            Ok(b) => (b, true),
            Err(_) => (dummy.to_vec(), false),
        };
        let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return false,
        };
        mac.update(body);
        mac.verify_slice(&expected_bytes).is_ok() && hex_valid
    }

    #[test]
    fn signatures_pass_the_personas_verifier() {
        let secret = Secret::new("s3cret-for-tests");
        for body in [
            &b"{}"[..],
            b"{\"kind\":\"stream_end\"}",
            b"",
            &[0xff, 0x00, 0x10],
        ] {
            let sig = sign(&secret, body);
            assert!(sig.starts_with("sha256="));
            assert_eq!(sig.len(), "sha256=".len() + 64);
            assert!(personas_verify("s3cret-for-tests", body, &sig));
            // The prefix is optional on the Personas side.
            assert!(personas_verify(
                "s3cret-for-tests",
                body,
                sig.trim_start_matches("sha256=")
            ));
            assert!(!personas_verify("other-secret", body, &sig));
        }
        let sig = sign(&secret, b"{\"a\":1}");
        assert!(!personas_verify("s3cret-for-tests", b"{\"a\":2}", &sig));
        assert!(!personas_verify(
            "s3cret-for-tests",
            b"{\"a\":1}",
            "sha256=zz"
        ));
    }

    #[test]
    fn a_known_vector() {
        // RFC 4231 test case 2.
        let sig = sign(&Secret::new("Jefe"), b"what do ya want for nothing?");
        assert_eq!(
            sig,
            "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn the_secret_never_prints() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        let e = Secret::from_env("SHADOWRUNR_TEST_UNSET_VAR_8f3a").unwrap_err();
        assert!(e.contains("SHADOWRUNR_TEST_UNSET_VAR_8f3a"));
    }
}
