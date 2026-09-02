$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force assets | Out-Null

Invoke-WebRequest `
  'https://raw.githubusercontent.com/Xiaobin-Rong/gtcrn/main/stream/onnx_models/gtcrn_simple.onnx' `
  -OutFile 'assets/gtcrn_simple.onnx'

Invoke-WebRequest `
  'https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx' `
  -OutFile 'assets/silero_vad.onnx'

Write-Host "Models downloaded."
