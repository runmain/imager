# Imager

Imager is a desktop client for AI image generation and chat. You bring your own API providers (any OpenAI-compatible gateway, or Anthropic's Claude API). Generated images are saved as local files, and conversations are kept in a local SQLite database.

## Features

### Image generation

- Send a text prompt to any OpenAI-compatible `/images/generations` endpoint.
- Choose a preset size (1024 × 1024, 2048 × 2048, 4096 × 4096) or enter a custom width and height (1–8192).
- Images come back as base64 data or download links. Either way they are saved to a local folder (`~/.images/` by default) and shown from disk.
- When no image can be found in a response, the raw response is shown as text so you can see what went wrong.
- Preview images inside the app with zoom and pan, copy them to the clipboard, open their folder, view the raw API response, or retry a prompt.
- Topics are titled automatically by a text model of your choice, and you can rename them at any time.

### Chat

- Stream multi-turn conversations using the OpenAI Responses API or the Anthropic Messages API.
- Create multiple assistants, each with its own system prompt and its own list of conversations.
- Replies are rendered as Markdown: headings, lists, tables, quotes, links, and syntax-highlighted code blocks with a copy button.

### General

- Manage several providers (name, base URL, API key, type). Fetch each provider's model list and keep only the models you use.
- Choose the interface font and font size.
- On macOS, closing the window hides the app to the menu bar.

The user interface is in Chinese.

## Data storage

- Settings, providers, and chat history are stored in `~/.config/imageclient/imageclient.db`. The file is created with `0600` permissions.
- API keys are stored only in this local database.
- To use a different database file, set the `IMAGECLIENT_DB` environment variable.
- Generated images go to `~/.images/` by default. You can change this in Settings → 偏好设置 (Preferences).

## Build and run

Requires a recent stable Rust toolchain (developed with Rust 1.98, edition 2024).

```sh
cargo run --release   # build and start the app
cargo test            # run the test suite (uses local mock servers, no network access)
```

To package a macOS app bundle at `dist/Imager.app`:

```sh
./build.sh                  # current architecture
./build.sh --arch aarch64   # or x86_64
```
