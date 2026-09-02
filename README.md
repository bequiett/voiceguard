# VoiceGuard

Experimental low-latency mic cleanup plugin for Windows.

The main goal is simple: keep speech intact and get rid of as much junk around it as possible without adding a huge lookahead. It is aimed at Discord/open-mic use, not offline restoration.

## What it tries to remove

- fan and airflow noise
- keyboard clicks and other short transients
- coughs, sneezes and nose blowing
- burps and heavy breathing when they are clearly not speech
- bumps/bangs
- mic pops and plosives

It will deliberately let some questionable sounds through rather than chop words apart. There is no perfect classifier for every human/non-human sound in real time.

## Signal path

GTCRN does the continuous noise reduction. Silero VAD is used as a speech hint, then a small spectral detector handles obvious bursts/transients and plosives. The final gate is biased toward preserving speech.

At 48 kHz the processing hop is 768 samples, so the plugin latency is about 16 ms. There is no extra long lookahead.

## Controls

- **Strength**: amount of GTCRN noise reduction
- **Voice Protect**: backs off suppression when speech is likely
- **Artifact**: how hard burst/transient detection is allowed to act
- **Floor**: minimum gain while the gate is closed
- **Air**: amount of high-frequency content kept above the GTCRN band
- **Bypass**: dry signal

Defaults are intentionally conservative:

- Strength 72%
- Voice Protect 82%
- Artifact 70%
- Floor -32 dB
- Air 75%

## Build

Models are fetched during the build. On Windows:

```powershell
./scripts/fetch-models.ps1
cargo xtask bundle voiceguard --release
```

The bundle ends up at:

```text
target/bundled/VoiceGuard.vst3
```

The GitHub Actions workflow does the same build and runs pluginval before uploading the VST3 artifact.

For Discord, 48 kHz is the intended sample rate.

## Quick test

Use your normal mic position and check speech first. Then try keyboard, fan, cough, burp, nose blow, yawn and plosives one at a time. If speech gets clipped, raise Voice Protect. If speech is safe but obvious junk gets through, raise Artifact.

## License

AGPL-3.0-only.
