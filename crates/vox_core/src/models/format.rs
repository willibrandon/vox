//! Model file format detection via magic byte inspection.
//!
//! Validates model files by reading the first 4 bytes and matching against known
//! magic byte patterns for GGUF, GGML, and ONNX formats. Maps detected formats
//! to model slots (VAD/ASR/LLM) to enable safe model swapping.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, ensure};

/// Detected model file format based on magic byte inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// GGUF format (magic: `GGUF` / 0x47475546). Used by LLM models (Qwen).
    Gguf,
    /// GGML/GGMF/GGJT format. Used by ASR models (Whisper).
    Ggml,
    /// ONNX protobuf format (first byte 0x08). Used by VAD models (Silero).
    Onnx,
    /// Unrecognized file format.
    Unknown,
}

/// Detect the model file format by reading the first 4 bytes.
///
/// Returns [`ModelFormat::Unknown`] if the file header does not match any
/// known magic bytes. Returns an error if the file is smaller than 4 bytes
/// or cannot be read.
pub fn detect_format(path: &Path) -> Result<ModelFormat> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open model file: {}", path.display()))?;

    let file_len = file
        .metadata()
        .with_context(|| format!("failed to read metadata: {}", path.display()))?
        .len();
    ensure!(file_len >= 4, "file too small for format detection ({file_len} bytes): {}", path.display());

    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .with_context(|| format!("failed to read magic bytes: {}", path.display()))?;

    // GGUF: "GGUF" = 0x47 0x47 0x55 0x46
    if magic == [0x47, 0x47, 0x55, 0x46] {
        return Ok(ModelFormat::Gguf);
    }

    // GGML variants: "ggml" / "ggmf" / "ggjt"
    if magic == [0x67, 0x67, 0x6D, 0x6C]   // ggml
        || magic == [0x67, 0x67, 0x6D, 0x66] // ggmf
        || magic == [0x67, 0x67, 0x6A, 0x74] // ggjt
    {
        return Ok(ModelFormat::Ggml);
    }

    // ONNX: protobuf wire format — first byte 0x08 (field 1, varint type)
    if magic[0] == 0x08 {
        return Ok(ModelFormat::Onnx);
    }

    Ok(ModelFormat::Unknown)
}

/// Maps a [`ModelFormat`] to the model slot index it can fill.
///
/// Returns the index into [`MODELS`](super::MODELS):
/// - `0` = VAD (ONNX)
/// - `1` = ASR (GGML)
/// - `2` = LLM (GGUF)
/// - `None` for Unknown format.
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
    use tempfile::TempDir;

    fn write_magic(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write test file");
        path
    }

    #[test]
    fn test_gguf_magic_bytes() {
        let dir = TempDir::new().unwrap();
        // GGUF magic: 0x47475546 + some padding
        let path = write_magic(dir.path(), "model.gguf", &[0x47, 0x47, 0x55, 0x46, 0x00, 0x00]);
        assert_eq!(detect_format(&path).unwrap(), ModelFormat::Gguf);
        assert_eq!(format_to_slot(ModelFormat::Gguf), Some(2));
    }

    #[test]
    fn test_ggml_magic_bytes() {
        let dir = TempDir::new().unwrap();
        let path = write_magic(dir.path(), "model_ggml.bin", &[0x67, 0x67, 0x6D, 0x6C, 0x00]);
        assert_eq!(detect_format(&path).unwrap(), ModelFormat::Ggml);
        assert_eq!(format_to_slot(ModelFormat::Ggml), Some(1));
    }

    #[test]
    fn test_ggmf_magic_bytes() {
        let dir = TempDir::new().unwrap();
        let path = write_magic(dir.path(), "model_ggmf.bin", &[0x67, 0x67, 0x6D, 0x66, 0x00]);
        assert_eq!(detect_format(&path).unwrap(), ModelFormat::Ggml);
    }

    #[test]
    fn test_ggjt_magic_bytes() {
        let dir = TempDir::new().unwrap();
        let path = write_magic(dir.path(), "model_ggjt.bin", &[0x67, 0x67, 0x6A, 0x74, 0x00]);
        assert_eq!(detect_format(&path).unwrap(), ModelFormat::Ggml);
    }

    #[test]
    fn test_onnx_protobuf_byte() {
        let dir = TempDir::new().unwrap();
        // ONNX protobuf: first byte 0x08 (field 1, varint)
        let path = write_magic(dir.path(), "model.onnx", &[0x08, 0x07, 0x12, 0x04, 0x00]);
        assert_eq!(detect_format(&path).unwrap(), ModelFormat::Onnx);
        assert_eq!(format_to_slot(ModelFormat::Onnx), Some(0));
    }

    #[test]
    fn test_unknown_random_bytes() {
        let dir = TempDir::new().unwrap();
        let path = write_magic(dir.path(), "random.dat", &[0xFF, 0xFE, 0xFD, 0xFC, 0x00]);
        assert_eq!(detect_format(&path).unwrap(), ModelFormat::Unknown);
        assert_eq!(format_to_slot(ModelFormat::Unknown), None);
    }

    #[test]
    fn test_file_too_small() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tiny.bin");
        std::fs::write(&path, [0x47, 0x47]).unwrap();
        let result = detect_format(&path);
        assert!(result.is_err(), "should error on files smaller than 4 bytes");
    }
}
