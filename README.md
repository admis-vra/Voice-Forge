# VoiceForge

**Universal voice typing for Windows.** Hold a hotkey, speak, release — your words are
typed into whatever application currently has keyboard focus. Not a chatbot, not a note
app: a keyboard replacement for your voice.

> Speech → Text → Type. Nothing else.

VoiceForge runs quietly in the system tray. While idle it uses no microphone and no
network — audio is only captured while the hotkey is held.

## How it works

```
Hold hotkey → capture mic → stream to Deepgram → release → final transcript → type at cursor → idle
```

- **Global push-to-talk hotkey** (default **Alt + Space**), configurable.
- **Offline speech recognition** by default via **local Whisper** (`whisper.cpp`) — no
  API key, no internet, no account. The model downloads itself once, then everything runs
  on your machine. Cloud providers (**OpenAI**, **Deepgram**) remain selectable in
  Settings.
- **Direct text injection** with `SendInput` (Unicode), so it works in editors,
  browsers, terminals, chat apps, forms — anywhere you can type. A clipboard-paste
  fallback preserves and restores your existing clipboard.
- **System tray** presence with status glyph (blue idle, red while listening).
- **Settings window**: microphone, language, hotkey, provider, launch-at-startup, and
  the Deepgram API key.

## Requirements

**To run** (default, offline): nothing but Windows 10/11 and a microphone. On first use
VoiceForge downloads a Whisper model (~142 MB for `base`) once; after that it works with
no internet and no API key.

**To build from source:**
- Rust toolchain (MSVC) — `rustup` with `x86_64-pc-windows-msvc`.
- Visual Studio 2022 with the **C++ build tools** and **CMake** (to compile whisper.cpp).
- **LLVM** (for `libclang`, used to generate the Whisper bindings) —
  `winget install LLVM.LLVM`.

The build reads [.cargo/config.toml](.cargo/config.toml), which points `LIBCLANG_PATH`
at `C:\Program Files\LLVM\bin` and pins the CMake generator to *Visual Studio 17 2022*.
Adjust those if your paths differ. Then just `cargo run`.

**Optional cloud providers:** an [OpenAI](https://platform.openai.com/api-keys) or
[Deepgram](https://deepgram.com) key (selectable in Settings). A **Mock** provider is also
included for offline pipeline testing.

## Build & run

```sh
cargo run            # debug build, logs to a console window
cargo build --release
./target/release/voiceforge.exe
```

## First-time setup

1. Launch VoiceForge — it appears in the system tray. On first run it downloads the
   local Whisper model in the background (watch the log / dashboard for progress).
2. Once the model is ready, put your cursor in any text field, hold **Alt + Space**,
   speak, and release. That's it — no key, no account, no internet.
3. Optionally right-click the tray → **Settings…** to change the microphone, language,
   hotkey, or Whisper model.

**Prefer a cloud provider?** In Settings set **Speech provider → OpenAI** (or Deepgram),
paste your key, and **Save key** (stored in **Windows Credential Manager**, never in a
file). A **Mock** provider types a placeholder for offline pipeline testing.

## Where things live

- Config: `%APPDATA%\VoiceForge\VoiceForge\config\config.toml`
- Whisper models: `…\config\models\` (e.g. `ggml-base.bin`)
- Logs: `…\config\logs\voiceforge.log` (daily rotation)
- API key (cloud providers only): Windows Credential Manager (service `VoiceForge`)

## Architecture

Each external concern sits behind its own module so the controller depends only on
abstractions — this keeps the door open for macOS/Linux and additional STT providers.

| Module        | Responsibility                                                   |
|---------------|------------------------------------------------------------------|
| `config`      | User preferences, persisted as TOML                              |
| `secrets`     | API key in the OS credential vault                               |
| `hotkey`      | Global push-to-talk via a low-level keyboard hook                |
| `audio`       | Microphone capture (cpal), down-mixed to mono 16-bit PCM         |
| `stt`         | Speech providers behind one interface (`whisper`, `openai`, `deepgram`, `mock`) |
| `stt::model`  | Local Whisper model detection, auto-download, and storage        |
| `inject`      | `SendInput` Unicode typing + clipboard fallback                  |
| `controller`  | Orchestrates press → capture → transcribe → release → type       |
| `tray` / `ui` | Tray icon + egui settings window                                 |
| `autostart`   | Optional launch-at-sign-in (Run registry key)                    |

## Privacy

By default (local Whisper) audio **never leaves your machine** — transcription happens
entirely offline. Audio is captured only while the hotkey is held and is never recorded to
disk. If you opt into a cloud provider (OpenAI/Deepgram), audio is streamed only to that
provider while you hold the key.

## License

MIT
