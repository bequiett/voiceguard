$ErrorActionPreference = 'Stop'
$modelDir = 'assets/dfn3_h0'
New-Item -ItemType Directory -Force $modelDir | Out-Null
$base = 'https://raw.githubusercontent.com/shimondoodkin/deepfilter-rt/f1a1b27be767a5e62ccc8625017f1737c33a3bf9/models/dfn3_h0'
$files = @(
  'config.ini',
  'combined_streaming.onnx',
  'enc_conv_streaming.onnx',
  'enc_gru_streaming.onnx',
  'erb_dec_streaming.onnx',
  'df_dec_streaming.onnx'
)
foreach ($file in $files) {
  Invoke-WebRequest "$base/$file" -OutFile "$modelDir/$file"
}
Write-Host 'DFN3-H0 models downloaded.'
