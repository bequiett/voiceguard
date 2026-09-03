$ErrorActionPreference = 'Stop'
$modelDir = 'assets/dpdfnet8'
New-Item -ItemType Directory -Force $modelDir | Out-Null
$model = 'https://huggingface.co/Ceva-IP/DPDFNet/resolve/main/onnx/dpdfnet8_48khz_hr.onnx?download=true'
Invoke-WebRequest $model -OutFile "$modelDir/dpdfnet8_48khz_hr.onnx"
Write-Host 'DPDFNet8 48 kHz HR model downloaded.'
