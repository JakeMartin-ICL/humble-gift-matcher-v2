# Humble Gift Matcher v2

A local-first desktop application that matches unrevealed Steam entitlements
from a user's Humble Bundle library with games wishlisted by the user and their
Steam friends.

The current implementation includes:

- a Tauri 2, React, TypeScript, and Rust application foundation;
- Steam sign-in by QR approval through Steam Mobile;
- Steam refresh-token reconnection in development builds;
- a dedicated, capability-free Humble login WebView;
- capture of Humble's secure `_simpleauth_sess` cookie from the native WebView
  cookie store;
- Humble order loading through the retained native session, including complete
  authenticated Choice-month catalogues so unrevealed monthly games are not
  omitted from matching;
- a key-free local entitlement cache that expires after 24 hours, with an
  explicit Humble refresh action;
- key-free entitlement normalization with explicit available, revealed,
  excluded, and needs-mapping classifications;
- complete entitlement rendering with lazy-loaded Steam header artwork;
- an entitlement-discovery view ranked by lifetime Steam user-review percentage
  or total review volume, loaded on demand and cached for seven days, with
  ownership badges for games already in the signed-in Steam library;
- on-demand entitlement detail drawers with Steam descriptions, genres,
  screenshots, review summaries, and native Steam/Humble actions;
- loading of the signed-in account's friends and every accessible Steam
  wishlist;
- AppID-first wishlist matching, followed by conservative local title matching
  for older Humble records that omit an AppID;
- Steam-title validation for Humble-supplied AppIDs, correcting obvious
  mismatches through the same conservative mapping pipeline;
- fuzzy local Steam mapping suggestions, automatic unique exact-title matches
  from Steam Store search, on-demand title search, and locally persisted
  corrections;
- an explicitly labelled manual Humble-session fallback for development;
- a responsive two-account onboarding flow and authenticated application shell;
  and
- tagged cross-platform release builds for macOS, Windows, and Linux.

This repository succeeds
[`JakeMartin-ICL/humble-gift-matcher`](https://github.com/JakeMartin-ICL/humble-gift-matcher).
The original Python repository is intentionally preserved as a separate
historical project.

Read the [project brief](docs/v2-spec.md) before implementation. Build the
actual application from the outset, validating the currently untested Steam and
Humble integration details as part of normal development.

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

## Temporary development credential cache

Debug builds currently retain the Steam refresh/access tokens and Humble
session cookie in `dev-credentials.json` under the operating system's local
application-data directory. On Unix, the directory and file are restricted to
the current user. Credential values are never returned to frontend state or
written to logs.

This cache is deliberately unavailable in release builds. It must be replaced
with the operating system credential store before this repository is pushed or
the application is distributed.

Disconnecting either account removes its retained value. When both accounts
have been disconnected, the cache file is deleted.

Steam app-title metadata, corrected Humble-to-Steam mappings, and normalized
key-free Humble entitlements are cached separately in the same local
application-data directory. Humble titles are first compared against accessible
wishlist metadata on-device. Unresolved titles are then searched against Steam
Store automatically; only a unique normalized 100% match is accepted, while
the remaining results stay available for review in the mapping editor.

The application is unofficial and is not affiliated with Humble Bundle or
Valve Corporation.
