# VoiceGuard

Low-latency microphone cleanup VST3 for Windows.

## v0.3

VoiceGuard 0.3 uses the 48 kHz DPDFNet8 high-resolution streaming model for the main speech enhancement stage. The plugin keeps a short event lookahead for impacts, breath and wind handling instead of trading most of the quality budget for minimum latency.

The current target is 48 kHz with 2880 samples (60 ms) reported latency.

## Controls

- **Strength**: DPDFNet8 wet amount
- **Voice Protect**: limits extra event attenuation
- **Artifact**: transient, breath and wind suppression
- **Floor**: lowest event attenuation level
- **Bypass**: dry signal

## Build

```powershell
./scripts/fetch-models.ps1
cargo xtask bundle voiceguard --release
```

GitHub Actions runs the release checks, tests and pluginval validation before uploading the VST3 artifact.

## Notes

A single microphone cannot reliably identify a phone speaker as a separate source when it contains ordinary human speech. v0.3 treats playback leaking into the mic as competing speech/noise and relies on the enhancement model where separation is possible. Reference-based acoustic echo cancellation would require the playback signal itself as an additional input.

## License

VoiceGuard is AGPL-3.0-only. DPDFNet model/code attribution remains under Apache-2.0.
