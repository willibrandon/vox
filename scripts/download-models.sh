#!/usr/bin/env bash
# Download ML models and ONNX Runtime for Vox.
# Usage: ./scripts/download-models.sh
#
# Downloads all required files to their expected locations with
# SHA-256 verification. Skips files that already exist with correct
# checksums. Runs on both macOS and Linux.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES_DIR="$REPO_ROOT/crates/vox_core/tests/fixtures"
VENDOR_ORT_DIR="$REPO_ROOT/vendor/onnxruntime"
ORT_VERSION="1.23.0"

# --- Helpers ---

sha256_check() {
    local file="$1" expected="$2"
    local actual
    if command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "$file" | awk '{print $1}')"
    elif command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$file" | awk '{print $1}')"
    else
        echo "WARNING: no sha256 tool found, skipping verification for $file"
        return 0
    fi

    if [ "$actual" = "$expected" ]; then
        return 0
    else
        echo "CHECKSUM MISMATCH for $file"
        echo "  expected: $expected"
        echo "  actual:   $actual"
        return 1
    fi
}

download_file() {
    local url="$1" dest="$2" checksum="$3" name="$4"

    if [ -f "$dest" ] && sha256_check "$dest" "$checksum" 2>/dev/null; then
        echo "[skip] $name — already exists with correct checksum"
        return 0
    fi

    echo "[download] $name → $(basename "$dest")"
    local tmp="${dest}.tmp"
    curl -fL --progress-bar -o "$tmp" "$url"
    if ! sha256_check "$tmp" "$checksum"; then
        rm -f "$tmp"
        echo "FATAL: $name download failed checksum verification"
        return 1
    fi
    mv "$tmp" "$dest"
    echo "[ok] $name"
}

detect_ort_url() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Darwin)
            case "$arch" in
                arm64)  echo "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-osx-arm64-${ORT_VERSION}.tgz" ;;
                x86_64) echo "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-osx-x86_64-${ORT_VERSION}.tgz" ;;
                *) return 1 ;;
            esac
            ;;
        Linux)
            case "$arch" in
                x86_64)  echo "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-x64-${ORT_VERSION}.tgz" ;;
                aarch64) echo "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-aarch64-${ORT_VERSION}.tgz" ;;
                *) return 1 ;;
            esac
            ;;
        *) return 1 ;;
    esac
}

ort_dylib_name() {
    case "$(uname -s)" in
        Darwin)          echo "libonnxruntime.dylib" ;;
        Linux)           echo "libonnxruntime.so" ;;
        MINGW*|MSYS*|CYGWIN*) echo "onnxruntime.dll" ;;
        *)               echo "libonnxruntime.so" ;;
    esac
}

# --- Main ---

echo "=== Vox Model Downloader ==="
echo ""

mkdir -p "$FIXTURES_DIR" "$VENDOR_ORT_DIR"

# --- ONNX Runtime ---

ORT_URL="$(detect_ort_url)" || {
    echo "FATAL: unsupported platform $(uname -s)/$(uname -m) for ONNX Runtime"
    exit 1
}
ORT_DYLIB_NAME="$(ort_dylib_name)"
ORT_DYLIB_PATH="$VENDOR_ORT_DIR/$ORT_DYLIB_NAME"

if [ -f "$ORT_DYLIB_PATH" ]; then
    echo "[skip] ONNX Runtime $ORT_VERSION — already installed"
else
    echo "[download] ONNX Runtime $ORT_VERSION"
    ORT_TMP="$(mktemp)"
    curl -fL --progress-bar -o "$ORT_TMP" "$ORT_URL"
    tar -xzf "$ORT_TMP" -C /tmp/
    ORT_EXTRACTED="$(ls -d /tmp/onnxruntime-*-${ORT_VERSION} 2>/dev/null | head -1)"
    if [ -z "$ORT_EXTRACTED" ]; then
        echo "FATAL: failed to extract ONNX Runtime archive"
        rm -f "$ORT_TMP"
        exit 1
    fi
    cp "$ORT_EXTRACTED/lib/$ORT_DYLIB_NAME" "$ORT_DYLIB_PATH"
    for versioned in "$ORT_EXTRACTED"/lib/libonnxruntime.*.dylib; do
        [ -f "$versioned" ] && cp "$versioned" "$VENDOR_ORT_DIR/"
    done
    rm -f "$ORT_TMP"
    rm -rf "$ORT_EXTRACTED"
    echo "[ok] ONNX Runtime $ORT_VERSION"
fi

echo ""

# --- ML Models (concurrent downloads) ---

download_file \
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx" \
    "$FIXTURES_DIR/silero_vad_v5.onnx" \
    "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3" \
    "Silero VAD v5" &
PID_VAD=$!

download_file \
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin" \
    "$FIXTURES_DIR/ggml-large-v3-turbo-q5_0.bin" \
    "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2" \
    "Whisper Large V3 Turbo (q5_0)" &
PID_WHISPER=$!

download_file \
    "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf" \
    "$FIXTURES_DIR/qwen2.5-3b-instruct-q4_k_m.gguf" \
    "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d" \
    "Qwen 2.5 3B Instruct (Q4_K_M)" &
PID_QWEN=$!

ALL_OK=true
wait $PID_VAD   || { echo "FATAL: Silero VAD download failed"; ALL_OK=false; }
wait $PID_WHISPER || { echo "FATAL: Whisper download failed"; ALL_OK=false; }
wait $PID_QWEN   || { echo "FATAL: Qwen download failed"; ALL_OK=false; }

echo ""

# --- Speech test fixture ---

SPEECH_WAV="$FIXTURES_DIR/speech_sample.wav"
if [ -f "$SPEECH_WAV" ]; then
    echo "[skip] speech_sample.wav — already exists"
elif command -v say >/dev/null 2>&1 && command -v ffmpeg >/dev/null 2>&1; then
    echo "[generate] speech_sample.wav via macOS TTS"
    TMP_AIFF="$(mktemp).aiff"
    say -o "$TMP_AIFF" "The quick brown fox jumps over the lazy dog. This is a test of the voice activity detection system. One two three four five six seven eight nine ten."
    ffmpeg -y -loglevel error -i "$TMP_AIFF" -ar 16000 -ac 1 -sample_fmt s16 "$SPEECH_WAV"
    rm -f "$TMP_AIFF"
    echo "[ok] speech_sample.wav"
else
    echo "FATAL: cannot generate speech_sample.wav — requires macOS (say + ffmpeg)"
    ALL_OK=false
fi

echo ""

if $ALL_OK; then
    echo "=== All downloads complete ==="
else
    echo "=== Some downloads failed — see errors above ==="
    exit 1
fi
