//! Streamed download with SHA-256 verification.

use sha2::{Digest, Sha256};
use std::io::Read;

/// Copy data from `reader` to `writer` while computing a SHA-256 hash.
///
/// Uses a 64 KiB buffer. Returns the lowercase hex digest.
pub fn copy_hashed(
    reader: &mut impl Read,
    writer: &mut impl std::io::Write,
) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        writer.write_all(&buffer[..n])?;
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_values() {
        // sha256("abc")
        let input = b"abc";
        let mut writer = std::io::sink();
        let hash = copy_hashed(&mut &input[..], &mut writer).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        // sha256("")
        let input = b"";
        let mut writer = std::io::sink();
        let hash = copy_hashed(&mut &input[..], &mut writer).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn writer_receives_exact_bytes() {
        let input = b"hello world";
        let mut writer = Vec::new();
        let _hash = copy_hashed(&mut &input[..], &mut writer).unwrap();
        assert_eq!(writer, input.as_slice());
    }
}
