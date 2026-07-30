# Humble Gift Matcher v2 — Project Brief

Status: initial handoff brief

Date: 2026-07-30

Legacy repository: <https://github.com/JakeMartin-ICL/humble-gift-matcher>

Legacy local path at handoff:
`/Users/jakemartin/Projects/humble-gift-matcher`

## 1. Purpose

Humble Gift Matcher helps someone find good recipients for spare game keys. It
compares the Steam games available in their Humble Bundle account with the
wishlists of their Steam friends and themselves.

The legacy Python terminal application:

- accepts a Humble `_simpleauth_sess` cookie;
- retrieves the user's Humble orders and entitlement metadata from Humble's
  private account endpoints;
- asks for Steam credentials, a Web API key, and a SteamID;
- retrieves the user's Steam friends and their wishlists;
- maps Humble products to Steam AppIDs; and
- prints each friend's matching games.

This repository is a clean successor rather than a migration. Preserve the
legacy repository as a historical project.

## 2. Product direction

Build a local-first Tauri 2 desktop application for Windows, macOS, and Linux.
It should provide a friendly visual workflow and require no application
backend.

The intended connection experience is:

- Steam QR login through the official Steam Mobile application; and
- a real Humble login page shown in a dedicated application window.

Users should not have to find a SteamID, create a Steam Web API key, enter a
Steam password into this application, or copy a Humble cookie from browser
developer tools during the normal flow.

The application is read-only with respect to both accounts. It should not:

- reveal, redeem, assign, email, or otherwise transmit game keys;
- buy or modify Humble products;
- change wishlists, friends, or other Steam account data; or
- require the user to upload account data to a service operated by this
  project.

A match should direct the user back to Humble to reveal or gift the entitlement
themselves.

## 3. Desired user experience

A typical session should be:

1. Connect Steam by scanning and approving a QR code.
2. Sign into Humble in an embedded window if no valid session is available.
3. Load eligible Humble Steam entitlements.
4. Load the user's Steam friends and accessible wishlists.
5. Display who wants each available game, including matches for the user.
6. Let the user filter, sort, search, inspect uncertain mappings, and open the
   relevant Humble purchase page.

Useful views may include:

- games grouped by the number of interested recipients;
- a person's complete set of matches;
- unmatched or ambiguous Humble products needing confirmation;
- friends whose wishlists are inaccessible; and
- summary counts for available entitlements, friends checked, and matches.

The final information design is open. It should work well with large Humble
libraries and Steam friend lists, remain keyboard accessible, and explain
partial results rather than silently omitting them.

## 4. Steam integration

Use a local Steam client-protocol session with QR approval. The previous Steam
Storage Optimiser v2 work may offer reusable knowledge or code, but this
project should not assume that the two applications must share a repository or
release lifecycle.

The Steam integration needs:

- the authenticated user's SteamID and display identity;
- accepted Steam friends and their display identities;
- the user's own wishlist; and
- each friend's wishlist when its privacy settings permit access.

The friend list should come from the authenticated client session rather than a
user-supplied Web API key. Steam client implementations receive friend-list
state during login.

The legacy wishlist route,
`/wishlist/profiles/{steamid}/wishlistdata/`, should not be carried forward.
Steam now exposes `IWishlistService/GetWishlist`, which returns the AppIDs on an
accessible wishlist:

`https://api.steampowered.com/IWishlistService/GetWishlist/v1/?steamid=...`

Steam also has newer filtered/shared-wishlist methods and share-token concepts.
The exact behavior for public, friends-only, private, and self wishlists should
be explored while implementing the real integration. Expected behavior is:

- public wishlists can be read;
- the signed-in user's wishlist can be read;
- friends-only wishlists may require the authenticated Steam web session; and
- private wishlists remain unavailable.

An inaccessible wishlist is a normal state, not an empty wishlist. Show the
difference in the UI.

Steam login tokens and authenticated web cookies are sensitive. Keep them in
the native process, never in persisted frontend state or logs. If remembering a
Steam account is offered, make it explicit and use the operating system
credential store. Logout must disconnect and remove retained credentials.

If client-protocol QR login proves unsuitable on a target platform, automatic
SteamID detection plus a user-supplied Web API key is an acceptable fallback.
Do not handle a Steam password.

## 5. Humble integration

No documented public Humble account API or third-party OAuth flow was found as
of the date of this brief. The existing private endpoints still appear to be in
active use by independent tools:

```text
GET https://www.humblebundle.com/api/v1/user/order
GET https://www.humblebundle.com/api/v1/orders
GET https://www.humblebundle.com/api/v1/order/{order-key}
```

Order detail requests use `all_tpkds=true`; the bulk route accepts repeated
`gamekeys` parameters.

These endpoints are authenticated by Humble's `_simpleauth_sess` cookie. They
are unsupported and may change, so parsing and errors should be resilient and
make failures understandable.

### Embedded login

Open Humble's genuine login page in a dedicated Tauri WebView. The user enters
credentials directly into Humble's page and completes any 2FA, SSO, or CAPTCHA
there. After login, the native application can read the WebView cookie store
for `https://www.humblebundle.com`, including secure and `HttpOnly` cookies,
and use `_simpleauth_sess` in an in-memory HTTP client.

This is expected to work with current Tauri, but has not yet been tested against
Humble across all three desktop platforms. Explore details such as pop-up SSO,
navigation, CAPTCHA, cookie availability, expiry, and Linux WebKit behavior as
the login feature is built.

The Humble WebView is remote, untrusted content:

- give it no Tauri commands, plugins, or application capabilities;
- restrict navigation to appropriate Humble authentication pages and handle
  external links safely;
- close it after the session is acquired; and
- avoid sharing its WebView context with the main application where practical.

Manual `_simpleauth_sess` entry can remain as an advanced fallback if embedded
login fails. Do not import or decrypt cookies from the user's normal browser
profiles.

### Session security

The Humble cookie is a broadly privileged account session, not a scoped
read-only token.

- Never collect the user's Humble password directly.
- Never send the session to a project backend.
- Keep it in memory by default.
- Never log it, expose it to the frontend, include it in crash reports, or
  store it in plaintext.
- If persistent login is offered, require an explicit choice and use the
  operating system credential store.
- Allow the user to disconnect Humble and delete retained session data.
- Use only the read endpoints required for matching.

## 6. Humble entitlement model

The application is interested only in Steam entitlements that may still be
given away. Filter and retain enough metadata to explain every decision,
including where available:

- Humble order/game key;
- parent bundle or store purchase;
- human and machine names;
- `key_type` and `key_type_human_name`;
- `steam_app_id`;
- visibility and expiry information;
- region restrictions;
- whether a key value has been revealed; and
- a link back to the appropriate Humble purchase.

Use careful terminology:

- **Unrevealed** means `redeemed_key_val` is absent.
- **Revealed** means Humble has displayed a key value.
- Revealed does not prove that the key was redeemed on Steam.
- “Available” should have a clearly documented definition, initially expected
  to mean a visible, non-expired, unrevealed Steam entitlement.

Do not ingest, persist, or display actual key values. Deluxe editions, DLC,
packages, alternate storefront keys, and Humble Choice selections need
distinct handling. A Humble `steam_app_id` can identify only a base game even
when the entitlement includes additional content; surface that limitation
instead of claiming an exact package match.

## 7. Matching Humble products to Steam

AppID equality should drive matching:

1. Use a valid Humble-provided `steam_app_id`.
2. Reuse a previously confirmed local mapping.
3. For remaining products, offer likely Steam candidates for user
   confirmation.
4. Allow the user to mark an item as not a Steam game, intentionally ignored,
   or unresolved.

Avoid silently accepting fuzzy title matches. Normalized names are helpful for
suggestions but are unreliable around remasters, editions, franchises, DLC,
soundtracks, and similarly named games.

Store user-confirmed mappings locally without including account credentials or
game keys. Make mappings editable so mistakes can be corrected.

Wishlists contain AppIDs, so no price or full Steam catalogue data is required
for the core match. Fetch only the public store metadata needed for names,
artwork, and useful links, with sensible caching.

## 8. Match results and privacy

The core relationship is:

```text
eligible Humble Steam entitlement
  -> confirmed Steam AppID
  -> zero or more accessible wishlists containing that AppID
```

Results may include Steam display names and wishlist information belonging to
friends. Keep all matching local. Do not add telemetry that transmits account,
friend, wishlist, purchase, or entitlement data.

Possible useful actions are limited to local organization and navigation, such
as:

- hide or dismiss a candidate;
- mark a planned recipient locally;
- copy a game name or Steam store link;
- open the corresponding Steam or Humble page; and
- export a key-free summary after explicit user action.

Do not automate sending a gift or reveal a key as a side effect of choosing a
recipient.

## 9. Reliability and partial failure

The application depends on unsupported Humble endpoints and partially
documented Steam interfaces. Model failures explicitly:

- expired or invalid Humble session;
- Humble login or SSO not working in an embedded WebView;
- an endpoint or response shape changing;
- a Steam friend having no wishlist versus an inaccessible wishlist;
- rate limiting or temporary service failure;
- missing, invalid, or ambiguous Humble AppID metadata;
- an edition or DLC entitlement mapping only to its base game;
- duplicate entitlements for one AppID;
- region-restricted or expired keys;
- Humble Choice products not yet selected or represented in order data; and
- a Steam session disconnecting during a refresh.

Cache non-sensitive normalized results so temporary failures do not make the
application unusable, while clearly showing when data was last refreshed.
Never cache credentials alongside ordinary application data.

## 10. Implementation approach

Build the main Tauri application from the beginning and add complete vertical
slices to it. The Steam and Humble flows are credible implementation
directions, not prerequisites that must be proven in a separate prototype or
handed to another task.

Some integration behavior is necessarily untested. Investigate it as each
feature is implemented, record meaningful findings, and adjust the design when
real account behavior contradicts an assumption.

A sensible progression is:

- application shell and secure native/frontend boundary;
- Steam QR connection and friend identities;
- Humble embedded login and entitlement loading;
- wishlist retrieval and privacy-state handling;
- AppID mapping and matching;
- result exploration, local decisions, and polish; and
- cross-platform packaging.

This ordering is guidance rather than a mandated architecture or release plan.
Frontend framework, state management, Rust libraries, module layout, and
detailed visual design remain open.

Tests should emphasize pure entitlement parsing, state classification, AppID
mapping, and matching logic with sanitized fixtures. Live account integration
tests should be explicit, local, and incapable of revealing or mutating keys.

## 11. Distribution and terms

The application must state that it is unofficial and is not affiliated with
Humble Bundle or Valve.

Humble's account endpoints and cookie authentication are not a supported
third-party API. Their terms have historically restricted scraping and
automated access. Review the current terms before public distribution and
consider requesting permission from Humble. Keep access low-volume, user-
initiated, read-only, and limited to the user's own account, but do not imply
that these precautions make an unsupported integration officially authorized.

The Humble provider should be isolated enough that changes or removal do not
contaminate the matching domain and UI.

## 12. Open decisions

These should be resolved through normal product development:

- final display name and repository name;
- frontend framework and visual system;
- whether sessions are remembered by default, opt-in, or never persisted;
- whether the app supports revealed-but-redemption-unknown entitlements;
- exact handling of Humble Choice and DLC/package ambiguity;
- whether planned-recipient state is useful;
- export formats;
- refresh and caching policy;
- supported Steam wishlist privacy modes after real testing;
- signing, notarization, updates, and release channels; and
- license for the new repository.

## 13. References

- Active independent Humble client, still using manual
  `_simpleauth_sess` authentication:
  <https://github.com/smbl64/humble-cli>
- Its current Humble endpoint implementation:
  <https://github.com/smbl64/humble-cli/blob/master/internal/api/humble.go>
- Steam wishlist service:
  <https://steamapi.xpaw.me/IWishlistService>
- Steam Web API overview:
  <https://partner.steamgames.com/doc/webapi_overview>
- Tauri WebView cookie APIs:
  <https://docs.rs/tauri/latest/tauri/webview/struct.Webview.html>
- Tauri capabilities and remote-content security:
  <https://v2.tauri.app/security/capabilities/>

## 14. Guidance for the next Codex task

Read this brief and inspect the legacy code for behavioral context without
modifying the legacy repository. Then build the new application itself. Treat
the proposed integrations as the working design, test uncertain behavior while
implementing it, and keep the user informed when real Steam or Humble account
interaction is required.

Preserve the product intent and security boundaries above, but let evidence
shape the detailed architecture and user experience.
