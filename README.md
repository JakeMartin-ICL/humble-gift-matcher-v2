# Humble Gift Matcher v2

A local-first desktop application that matches unrevealed Steam entitlements
from a user's Humble Bundle library with games wishlisted by the user and their
Steam friends.

Highlights:

- find games you have keys for on Humble you can gift to friends or keep for 
  yourself from Steam wishlists
- rediscover forgotten Humble games by Steam rating or popularity
- inspect store details and correct uncertain Humble-to-Steam matches
- keep account data and matching entirely on your computer

This repository succeeds
[`JakeMartin-ICL/humble-gift-matcher`](https://github.com/JakeMartin-ICL/humble-gift-matcher).

## Installation

Download the latest version for your operating system from
[GitHub Releases](https://github.com/JakeMartin-ICL/humble-gift-matcher-v2/releases/latest).

- **macOS:** choose the Apple Silicon or Intel `.dmg`, then drag Humble Gift
  Matcher into Applications.
- **Windows:** download and run the Windows `.exe` installer.
- **Linux:** use the `.AppImage`, or install the `.deb` package on Debian-based
  distributions.

## Development

Prerequisites:

- Node.js and npm;
- a current stable Rust toolchain; and
- the platform dependencies required by Tauri 2.

Install dependencies and run the desktop application:

```sh
npm install
npm run tauri dev
```

Run the automated checks:

```sh
npm test
npm run build
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

Create a production bundle:

```sh
npm run tauri build
```

For rapid local iteration, debug builds can explicitly opt into the old
plaintext development cache:

```sh
HGM_INSECURE_DEV_CREDENTIAL_CACHE=1 npm run tauri dev
```

In PowerShell:

```powershell
$env:HGM_INSECURE_DEV_CREDENTIAL_CACHE = "1"
npm run tauri dev
```

The application is unofficial and is not affiliated with Humble Bundle or
Valve Corporation.
