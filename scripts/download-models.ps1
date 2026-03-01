# Download ML models and ONNX Runtime for Vox.
# Usage: .\scripts\download-models.ps1
#
# Downloads all required files to their expected locations with
# SHA-256 verification. Skips files that already exist with correct
# checksums. Windows-only (use download-models.sh on macOS/Linux).

$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$FixturesDir = Join-Path $RepoRoot 'crates\vox_core\tests\fixtures'
$VendorOrtDir = Join-Path $RepoRoot 'vendor\onnxruntime'
$OrtVersion = '1.23.0'

# --- Helpers ---

function Test-Sha256 {
    param(
        [string]$FilePath,
        [string]$Expected
    )
    $actual = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()
    if ($actual -eq $Expected) {
        return $true
    }
    Write-Host "CHECKSUM MISMATCH for $FilePath"
    Write-Host "  expected: $Expected"
    Write-Host "  actual:   $actual"
    return $false
}

function Get-ModelFile {
    param(
        [string]$Url,
        [string]$Dest,
        [string]$Checksum,
        [string]$Name
    )

    if ((Test-Path $Dest) -and (Test-Sha256 -FilePath $Dest -Expected $Checksum)) {
        Write-Host "[skip] $Name - already exists with correct checksum"
        return $true
    }

    Write-Host "[download] $Name -> $(Split-Path $Dest -Leaf)"
    $tmp = "$Dest.tmp"
    try {
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
    } catch {
        Write-Host "FATAL: failed to download $Name : $_"
        Remove-Item -Path $tmp -ErrorAction SilentlyContinue
        return $false
    }

    if (-not (Test-Sha256 -FilePath $tmp -Expected $Checksum)) {
        Remove-Item -Path $tmp -ErrorAction SilentlyContinue
        Write-Host "FATAL: $Name download failed checksum verification"
        return $false
    }

    Move-Item -Path $tmp -Destination $Dest -Force
    Write-Host "[ok] $Name"
    return $true
}

# --- Main ---

Write-Host "=== Vox Model Downloader ==="
Write-Host ""

New-Item -ItemType Directory -Path $FixturesDir -Force | Out-Null
New-Item -ItemType Directory -Path $VendorOrtDir -Force | Out-Null

# --- ONNX Runtime ---

$OrtUrl = "https://github.com/microsoft/onnxruntime/releases/download/v${OrtVersion}/onnxruntime-win-x64-${OrtVersion}.zip"
$OrtDllPath = Join-Path $VendorOrtDir 'onnxruntime.dll'

if (Test-Path $OrtDllPath) {
    Write-Host "[skip] ONNX Runtime $OrtVersion - already installed"
} else {
    Write-Host "[download] ONNX Runtime $OrtVersion"
    $ortTmp = Join-Path $env:TEMP "ort-${OrtVersion}.zip"
    $ortExtractDir = Join-Path $env:TEMP "ort-extract-${OrtVersion}"

    try {
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $OrtUrl -OutFile $ortTmp -UseBasicParsing
        Remove-Item -Path $ortExtractDir -Recurse -Force -ErrorAction SilentlyContinue
        Expand-Archive -Path $ortTmp -DestinationPath $ortExtractDir -Force

        $ortLibDir = Get-ChildItem -Path $ortExtractDir -Directory -Filter "onnxruntime-win-x64-*" |
            Select-Object -First 1
        if (-not $ortLibDir) {
            throw "failed to find extracted ONNX Runtime directory"
        }

        $dllSource = Join-Path $ortLibDir.FullName 'lib\onnxruntime.dll'
        if (-not (Test-Path $dllSource)) {
            throw "onnxruntime.dll not found in extracted archive"
        }
        Copy-Item -Path $dllSource -Destination $OrtDllPath -Force
        Write-Host "[ok] ONNX Runtime $OrtVersion"
    } catch {
        Write-Host "FATAL: ONNX Runtime download/extraction failed: $_"
        exit 1
    } finally {
        Remove-Item -Path $ortTmp -ErrorAction SilentlyContinue
        Remove-Item -Path $ortExtractDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""

# --- ML Models (concurrent downloads via background jobs) ---

$models = @(
    @{
        Name     = 'Silero VAD v5'
        Url      = 'https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx'
        Dest     = Join-Path $FixturesDir 'silero_vad_v5.onnx'
        Checksum = '1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3'
    },
    @{
        Name     = 'Whisper Large V3 Turbo (q5_0)'
        Url      = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin'
        Dest     = Join-Path $FixturesDir 'ggml-large-v3-turbo-q5_0.bin'
        Checksum = '394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2'
    },
    @{
        Name     = 'Qwen 2.5 3B Instruct (Q4_K_M)'
        Url      = 'https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf'
        Dest     = Join-Path $FixturesDir 'qwen2.5-3b-instruct-q4_k_m.gguf'
        Checksum = '626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d'
    }
)

$jobs = @()
foreach ($model in $models) {
    $jobs += Start-Job -ScriptBlock {
        param($Url, $Dest, $Checksum, $Name)

        $ErrorActionPreference = 'Stop'

        # Check existing file
        if (Test-Path $Dest) {
            $actual = (Get-FileHash -Path $Dest -Algorithm SHA256).Hash.ToLower()
            if ($actual -eq $Checksum) {
                return "[skip] $Name - already exists with correct checksum"
            }
        }

        # Download to temp
        $tmp = "$Dest.tmp"
        try {
            $ProgressPreference = 'SilentlyContinue'
            Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
        } catch {
            Remove-Item -Path $tmp -ErrorAction SilentlyContinue
            throw "download failed for $Name : $_"
        }

        # Verify checksum
        $actual = (Get-FileHash -Path $tmp -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $Checksum) {
            Remove-Item -Path $tmp -ErrorAction SilentlyContinue
            throw "checksum mismatch for $Name (expected $Checksum, got $actual)"
        }

        Move-Item -Path $tmp -Destination $Dest -Force
        return "[ok] $Name"
    } -ArgumentList $model.Url, $model.Dest, $model.Checksum, $model.Name
}

$allOk = $true
foreach ($job in $jobs) {
    $result = Receive-Job -Job $job -Wait
    if ($job.State -eq 'Failed') {
        Write-Host "FATAL: $($job.ChildJobs[0].JobStateInfo.Reason.Message)"
        $allOk = $false
    } else {
        Write-Host $result
    }
    Remove-Job -Job $job
}

Write-Host ""

# --- Speech test fixture ---

$speechWav = Join-Path $FixturesDir 'speech_sample.wav'
if (Test-Path $speechWav) {
    Write-Host "[skip] speech_sample.wav - already exists"
} else {
    # Use Windows Speech Synthesis + ffmpeg if available
    $ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($ffmpeg) {
        Write-Host "[generate] speech_sample.wav via Windows TTS"
        $tmpWav = Join-Path $env:TEMP 'vox_tts_raw.wav'
        try {
            Add-Type -AssemblyName System.Speech
            $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
            $synth.SetOutputToWaveFile($tmpWav)
            $synth.Speak("The quick brown fox jumps over the lazy dog. This is a test of the voice activity detection system. One two three four five six seven eight nine ten.")
            $synth.Dispose()

            & ffmpeg -y -loglevel error -i $tmpWav -ar 16000 -ac 1 -sample_fmt s16 $speechWav
            Remove-Item -Path $tmpWav -ErrorAction SilentlyContinue
            Write-Host "[ok] speech_sample.wav"
        } catch {
            Write-Host "WARNING: TTS generation failed: $_"
            Write-Host "  You can copy speech_sample.wav from another machine into $FixturesDir"
            Remove-Item -Path $tmpWav -ErrorAction SilentlyContinue
            $allOk = $false
        }
    } else {
        Write-Host "WARNING: speech_sample.wav not found and ffmpeg not available for generation"
        Write-Host "  Install ffmpeg or copy speech_sample.wav from another machine into $FixturesDir"
        $allOk = $false
    }
}

Write-Host ""

if ($allOk) {
    Write-Host "=== All downloads complete ==="
} else {
    Write-Host "=== Some downloads failed - see errors above ==="
    exit 1
}
