//! Is this the model file the whistle crate was tested against? The size and
//! SHA-256 pinned in `whistle::model_file`, checked on every copy before it
//! is used or cached: by the worker (`worker/model_load.rs`, with the
//! browser's digest) and natively (`native/model.rs`, with `sha2`).

use whistle::model_file::{MODEL_BYTES, MODEL_SHA256};

/// The outcome of a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    Ok,
    /// Some other file (size or digest differs).
    Mismatch,
}

/// The model's size in bytes.
#[cfg(any(target_arch = "wasm32", feature = "native-download", test))]
pub(crate) fn expected_bytes() -> usize {
    MODEL_BYTES as usize
}

pub(crate) fn size_ok(len: usize) -> bool {
    len as u64 == MODEL_BYTES
}

/// `bytes` as lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compare a SHA-256 (hex, either case) with the pinned one.
pub(crate) fn digest_check(sha256_hex: &str) -> Check {
    if sha256_hex.eq_ignore_ascii_case(MODEL_SHA256) {
        Check::Ok
    } else {
        Check::Mismatch
    }
}

/// The key a checked copy is cached under (the Cache API wants a URL): the
/// checksum, so another model version never reads this copy.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn cache_key() -> String {
    format!("https://agg-gui-whistle.invalid/{MODEL_SHA256}")
}

/// The pinned SHA-256, for native cache paths.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn expected_sha256() -> &'static str {
    MODEL_SHA256
}

/// Check `bytes` natively.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn check_bytes(bytes: &[u8]) -> Check {
    use sha2::{Digest, Sha256};
    if !size_ok(bytes.len()) {
        return Check::Mismatch;
    }
    digest_check(&hex(&Sha256::digest(bytes)))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lowercase_and_two_digits_a_byte() {
        assert_eq!(hex(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
    }

    #[test]
    fn the_pinned_digest_passes_in_either_case_and_others_fail() {
        assert_eq!(digest_check(MODEL_SHA256), Check::Ok);
        assert_eq!(digest_check(&MODEL_SHA256.to_uppercase()), Check::Ok);
        assert_eq!(digest_check(&"0".repeat(64)), Check::Mismatch);
        assert_eq!(digest_check(""), Check::Mismatch);
    }

    #[test]
    fn a_file_of_the_wrong_size_or_content_fails() {
        assert_eq!(check_bytes(b"not a model"), Check::Mismatch);
        assert_eq!(check_bytes(&vec![0u8; expected_bytes()]), Check::Mismatch);
    }

    #[test]
    fn the_real_model_passes_when_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../whistle-rust/models/whistle.cact");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipped: no ../whistle-rust/models/whistle.cact");
            return;
        };
        assert_eq!(check_bytes(&bytes), Check::Ok);
    }

    #[test]
    fn the_cache_key_names_the_checksum() {
        assert!(cache_key().ends_with(MODEL_SHA256));
    }
}
