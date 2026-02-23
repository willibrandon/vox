//! Model file format detection via magic byte inspection.
//!
//! Identifies whether a file is GGUF (LLM), GGML/GGMF/GGJT (ASR), or
//! ONNX (VAD) by reading the first 4 bytes. Used during model swapping
//! to validate that a user-supplied file matches the expected model slot.

use std::path::Path;

use anyhow::{Context, Result};

/// Detected model file format based on magic byte inspection.
///
/// Each format maps to exactly one model slot in the pipeline:
/// GGUF for LLM (Qwen), GGML variants for ASR (Whisper), ONNX for VAD (Silero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// GGUF format (magic: `GGUF` / `0x47475546`). Used by LLM models (Qwen).
    Gguf,
    /// GGML/GGMF/GGJT format. Used by ASR models (Whisper).
    Ggml,
    /// ONNX protobuf format (first byte `0x08`). Used by VAD models (Silero).
    Onnx,
    /// Unrecognized file format.
    Unknown,
}

/// Detect the model file format by reading the first 4 bytes.
///
/// Returns `ModelFormat::Unknown` if the header does not match any known
/// magic bytes. Returns an error if the file cannot be opened or is
/// smaller than 4 bytes.
pub fn detect_format(path: &Path) -> Result<ModelFormat> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {} for format detection", path.display()))?;

    let mut magic = [0u8; 4];
    let bytes_read = file
        .read(&mut magic)
        .with_context(|| format!("failed to read header from {}", path.display()))?;

    if bytes_read < 4 {
        anyhow::bail!(
            "file {} is too small ({} bytes) for format detection",
            path.display(),
            bytes_read
        );
    }

    // GGUF: 0x47475546 ("GGUF")
    if magic == [0x47, 0x47, 0x55, 0x46] {
        return Ok(ModelFormat::Gguf);
    }

    // GGML variants: "ggml", "ggmf", "ggjt"
    if magic == [0x67, 0x67, 0x6D, 0x6C]
        || magic == [0x67, 0x67, 0x6D, 0x66]
        || magic == [0x67, 0x67, 0x6A, 0x74]
    {
        return Ok(ModelFormat::Ggml);
    }

    // ONNX: protobuf wire format (field 1, varint type = 0x08)
    if magic[0] == 0x08 {
        return Ok(ModelFormat::Onnx);
    }

    Ok(ModelFormat::Unknown)
}

/// Map a detected model format to the MODELS registry slot index.
///
/// Returns `Some(0)` for VAD (ONNX), `Some(1)` for ASR (GGML),
/// `Some(2)` for LLM (GGUF), or `None` for unknown formats.
pub fn format_to_slot(format: ModelFormat) -> Option<usize> {
    match format {
        ModelFormat::Onnx => Some(0),
        ModelFormat::Ggml => Some(1),
        ModelFormat::Gguf => Some(2),
        ModelFormat::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_gguf() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.gguf");
        let mut data = vec![0x47u8, 0x47, 0x55, 0x46]; // "GGUF"
        data.extend_from_slice(&[0u8; 100]);
        std::fs::write(&path, &data).expect("write");

        assert_eq!(detect_format(&path).expect("detect"), ModelFormat::Gguf);
    }

    #[test]
    fn test_detect_ggml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.bin");
        let mut data = vec![0x67u8, 0x67, 0x6D, 0x6C]; // "ggml"
        data.extend_from_slice(&[0u8; 100]);
        std::fs::write(&path, &data).expect("write");

        assert_eq!(detect_format(&path).expect("detect"), ModelFormat::Ggml);
    }

    #[test]
    fn test_detect_ggmf() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.bin");
        let mut data = vec![0x67u8, 0x67, 0x6D, 0x66]; // "ggmf"
        data.extend_from_slice(&[0u8; 100]);
        std::fs::write(&path, &data).expect("write");

        assert_eq!(detect_format(&path).expect("detect"), ModelFormat::Ggml);
    }

    #[test]
    fn test_detect_ggjt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.bin");
        let mut data = vec![0x67u8, 0x67, 0x6A, 0x74]; // "ggjt"
        data.extend_from_slice(&[0u8; 100]);
        std::fs::write(&path, &data).expect("write");

        assert_eq!(detect_format(&path).expect("detect"), ModelFormat::Ggml);
    }

    #[test]
    fn test_detect_onnx() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.onnx");
        let mut data = vec![0x08u8, 0x00, 0x00, 0x00];
        data.extend_from_slice(&[0u8; 100]);
        std::fs::write(&path, &data).expect("write");

        assert_eq!(detect_format(&path).expect("detect"), ModelFormat::Onnx);
    }

    #[test]
    fn test_detect_unknown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.xyz");
        std::fs::write(&path, &[0xDE, 0xAD, 0xBE, 0xEF, 0x00]).expect("write");

        assert_eq!(detect_format(&path).expect("detect"), ModelFormat::Unknown);
    }

    #[test]
    fn test_detect_too_small() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tiny.bin");
        std::fs::write(&path, &[0x08, 0x00]).expect("write");

        let result = detect_format(&path);
        assert!(result.is_err(), "files < 4 bytes should return error");
    }

    #[test]
    fn test_format_to_slot_mapping() {
        assert_eq!(format_to_slot(ModelFormat::Onnx), Some(0));
        assert_eq!(format_to_slot(ModelFormat::Ggml), Some(1));
        assert_eq!(format_to_slot(ModelFormat::Gguf), Some(2));
        assert_eq!(format_to_slot(ModelFormat::Unknown), None);
    }
}
