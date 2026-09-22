import { invoke } from "@tauri-apps/api/core";
import {
  type FormEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import "./App.css";
import {
  initialAppView,
  type AppView,
  type Entitlement,
  type MappingCandidate,
  type SteamGameDetails,
} from "./types";

const refreshIntervalMs = 700;

function App() {
  const [view, setView] = useState<AppView>(initialAppView);
  const [commandError, setCommandError] = useState<string | null>(null);
  const humbleCheckPending = useRef(false);

  useEffect(() => {
    let disposed = false;
    const refresh = async () => {
      try {
        const next = await invoke<AppView>("get_app_view");
        if (!disposed) setView(next);

        if (
          next.humble.phase === "waiting" &&
          !humbleCheckPending.current
        ) {
          humbleCheckPending.current = true;
          void invoke("check_humble_login")
            .catch(() => undefined)
            .finally(() => {
              humbleCheckPending.current = false;
            });
        }
      } catch {
        // Browser-only tests and the Vite preview do not have Tauri IPC.
      }
    };

    void refresh();
    const interval = window.setInterval(refresh, refreshIntervalMs);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, []);

  const run = async (command: string, args?: Record<string, unknown>) => {
    setCommandError(null);
    try {
      await invoke(command, args);
      setView(await invoke<AppView>("get_app_view"));
    } catch (error) {
      setCommandError(String(error));
    }
  };
  const loadGameDetails = useCallback(
    (appId: number) =>
      invoke<SteamGameDetails>("load_steam_game_details", { appId }),
    [],
  );

  const connected =
    view.steam.phase === "connected" && view.humble.phase === "connected";

  return connected ? (
    <Workspace
      view={view}
      commandError={commandError}
      onDisconnectSteam={() => run("disconnect_steam")}
      onDisconnectHumble={() => run("disconnect_humble")}
      onRefreshEntitlements={() => run("refresh_humble_entitlements")}
      onCancelEntitlements={() => run("cancel_humble_refresh")}
      onLoadSteamReviews={() => run("load_steam_reviews")}
      onSetMapping={(mappingKey, appId, steamName) =>
        run("set_entitlement_mapping", { mappingKey, appId, steamName })
      }
      onClearMapping={(mappingKey) =>
        run("clear_entitlement_mapping", { mappingKey })
      }
      onSearchSteam={(query) =>
        invoke<MappingCandidate[]>("search_steam_apps", { query })
      }
      onLoadGameDetails={loadGameDetails}
      onOpenSteam={(appId) => run("open_steam_game", { appId })}
      onOpenHumble={(url) => run("open_humble_entitlement", { url })}
    />
  ) : (
    <Onboarding
      view={view}
      commandError={commandError}
      onConnectSteam={() => run("start_steam_login")}
      onCancelSteam={() => run("cancel_steam_login")}
      onDisconnectSteam={() => run("disconnect_steam")}
      onConnectHumble={() => run("start_humble_login")}
      onManualHumble={(session) =>
        run("connect_humble_with_cookie", { session })
      }
      onDisconnectHumble={() => run("disconnect_humble")}
    />
  );
}

function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <div className={`brand${compact ? " compact" : ""}`}>
      <span className="brand-mark" aria-hidden="true">
        <GiftMark />
      </span>
      <span className="brand-copy">
        <strong>Humble Gift Matcher</strong>
        <small>Find a home for every spare game</small>
      </span>
    </div>
  );
}

function GiftMark() {
  return (
    <svg viewBox="0 0 40 40" role="img" aria-label="Gift Matcher">
      <rect x="5.5" y="16" width="29" height="7" rx="1.8" />
      <path d="M8 25h11v10H8zM21 25h11v10H21z" />
      <path d="M20 16c-4.9-7.1-11.2-7.6-11.2-3.1 0 3.1 5.1 3.1 11.2 3.1Zm0 0c4.9-7.1 11.2-7.6 11.2-3.1 0 3.1-5.1 3.1-11.2 3.1Z" />
      <path className="ribbon" d="M20 15.5V35" />
    </svg>
  );
}

function Onboarding({
  view,
  commandError,
  onConnectSteam,
  onCancelSteam,
  onDisconnectSteam,
  onConnectHumble,
  onManualHumble,
  onDisconnectHumble,
}: {
  view: AppView;
  commandError: string | null;
  onConnectSteam: () => Promise<void>;
  onCancelSteam: () => Promise<void>;
  onDisconnectSteam: () => Promise<void>;
  onConnectHumble: () => Promise<void>;
  onManualHumble: (session: string) => Promise<void>;
  onDisconnectHumble: () => Promise<void>;
}) {
  const connectedCount =
    Number(view.steam.phase === "connected") +
    Number(view.humble.phase === "connected");

  return (
    <main className="onboarding-shell">
      <Brand compact />
      <section className="onboarding-layout">
        <header className="onboarding-intro">
          <p className="kicker">Local-first gift matching</p>
          <h1>Turn spare keys into thoughtful gifts.</h1>
          <p>
            Connect your accounts, then see which games in your Humble library
            are already on your friends&apos; Steam wishlists.
          </p>
          <div className="privacy-note">
            <ShieldIcon />
            <span>
              Account data stays on this Mac. Keys are never revealed,
              redeemed, or sent.
            </span>
          </div>
        </header>

        <section className="connection-panel" aria-live="polite">
          <div className="panel-heading">
            <div>
              <p className="kicker">Get started</p>
              <h2>Connect your libraries</h2>
            </div>
            <span className="step-count">{connectedCount} of 2</span>
          </div>

          <div className="progress-track" aria-hidden="true">
            <span style={{ width: `${connectedCount * 50}%` }} />
          </div>

          <div className="service-list">
            <SteamCard
              connection={view.steam}
              onConnect={onConnectSteam}
              onCancel={onCancelSteam}
              onDisconnect={onDisconnectSteam}
            />
            <HumbleCard
              connection={view.humble}
              developmentCache={view.developmentCache}
              onConnect={onConnectHumble}
              onManualConnect={onManualHumble}
              onDisconnect={onDisconnectHumble}
            />
          </div>

          {commandError && <p className="command-error">{commandError}</p>}

          <div className="next-step">
            <span className={connectedCount === 2 ? "ready" : ""}>
              {connectedCount === 2 ? "✓" : "3"}
            </span>
            <div>
              <strong>Find wishlist matches</strong>
              <small>
                {connectedCount === 2
                  ? "Your accounts are connected."
                  : "Connect both accounts to continue."}
              </small>
            </div>
          </div>
        </section>
      </section>
      <p className="onboarding-footnote">
        Unofficial and not affiliated with Humble Bundle or Valve Corporation.
      </p>
    </main>
  );
}

function SteamCard({
  connection,
  onConnect,
  onCancel,
  onDisconnect,
}: {
  connection: AppView["steam"];
  onConnect: () => Promise<void>;
  onCancel: () => Promise<void>;
  onDisconnect: () => Promise<void>;
}) {
  const busy = ["connecting", "qr_ready", "waiting"].includes(connection.phase);
  const connected = connection.phase === "connected";

  return (
    <article className={`service-card steam-card ${connection.phase}`}>
      <div className="service-icon steam-icon" aria-hidden="true">
        <SteamIcon />
      </div>
      <div className="service-copy">
        <div className="service-title">
          <h3>Steam</h3>
          <StatusPill phase={connection.phase} />
        </div>

        {connection.qrImage ? (
          <div className="qr-content">
            <img src={connection.qrImage} alt="Steam sign-in QR code" />
            <div>
              <strong>Approve in Steam Mobile</strong>
              <p>Open Steam Guard, scan the code, then approve this device.</p>
            </div>
          </div>
        ) : connected ? (
          <div className="connected-identity">
            <Avatar profile={connection.profile} />
            <span>
              <strong>{connection.profile?.displayName ?? "Steam account"}</strong>
              <small>
                {connection.remembered
                  ? "Session remembered on this device"
                  : "Connected for this session"}
              </small>
            </span>
          </div>
        ) : (
          <p className="service-message">
            {connection.error ?? connection.message}
          </p>
        )}
      </div>

      <div className="service-action">
        {connected ? (
          <button className="quiet-button" onClick={onDisconnect}>
            Disconnect
          </button>
        ) : busy ? (
          <button className="quiet-button" onClick={onCancel}>
            Cancel
          </button>
        ) : (
          <button className="primary-button" onClick={onConnect}>
            {connection.phase === "error" ? "Try again" : "Connect"}
            <span aria-hidden="true">→</span>
          </button>
        )}
      </div>
    </article>
  );
}

function HumbleCard({
  connection,
  developmentCache,
  onConnect,
  onManualConnect,
  onDisconnect,
}: {
  connection: AppView["humble"];
  developmentCache: boolean;
  onConnect: () => Promise<void>;
  onManualConnect: (session: string) => Promise<void>;
  onDisconnect: () => Promise<void>;
}) {
  const [showManual, setShowManual] = useState(false);
  const [session, setSession] = useState("");
  const connected = connection.phase === "connected";
  const busy = ["connecting", "waiting"].includes(connection.phase);

  const submitManual = async (event: FormEvent) => {
    event.preventDefault();
    if (!session.trim()) return;
    await onManualConnect(session.trim());
    setSession("");
  };

  return (
    <article className={`service-card humble-card ${connection.phase}`}>
      <div className="service-icon humble-icon" aria-hidden="true">
        H
      </div>
      <div className="service-copy">
        <div className="service-title">
          <h3>Humble Bundle</h3>
          <StatusPill phase={connection.phase} />
        </div>
        {connected ? (
          <div className="connected-identity">
            <span className="mini-check" aria-hidden="true">
              ✓
            </span>
            <span>
              <strong>Humble library connected</strong>
              <small>
                {connection.remembered
                  ? "Session remembered on this device"
                  : "Connected for this session"}
              </small>
            </span>
          </div>
        ) : (
          <>
            <p className="service-message">
              {connection.error ?? connection.message}
            </p>
            {developmentCache && !busy && (
              <button
                type="button"
                className="text-button"
                onClick={() => setShowManual((current) => !current)}
              >
                {showManual ? "Hide development fallback" : "Use a session cookie instead"}
              </button>
            )}
            {showManual && !busy && (
              <form className="manual-session" onSubmit={submitManual}>
                <label htmlFor="humble-session">
                  Development `_simpleauth_sess`
                </label>
                <div>
                  <input
                    id="humble-session"
                    type="password"
                    autoComplete="off"
                    value={session}
                    onChange={(event) => setSession(event.currentTarget.value)}
                    placeholder="Paste session value"
                  />
                  <button type="submit" disabled={!session.trim()}>
                    Save
                  </button>
                </div>
              </form>
            )}
          </>
        )}
      </div>

      <div className="service-action">
        {connected ? (
          <button className="quiet-button" onClick={onDisconnect}>
            Disconnect
          </button>
        ) : busy ? (
          <span className="waiting-indicator">
            <i />
            Waiting
          </span>
        ) : (
          <button className="primary-button" onClick={onConnect}>
            {connection.phase === "error" ? "Try again" : "Sign in"}
            <span aria-hidden="true">↗</span>
          </button>
        )}
      </div>
    </article>
  );
}

function StatusPill({ phase }: { phase: AppView["steam"]["phase"] }) {
  const label =
    phase === "connected"
      ? "Connected"
      : phase === "error"
        ? "Needs attention"
        : ["connecting", "qr_ready", "waiting"].includes(phase)
          ? "In progress"
          : "Not connected";
  return <span className={`status-pill ${phase}`}>{label}</span>;
}

function Avatar({ profile }: { profile: AppView["steam"]["profile"] }) {
  return (
    <span className="avatar">
      {profile?.displayName.slice(0, 1).toUpperCase() ?? "S"}
      {profile?.avatarUrl && (
        <img src={profile.avatarUrl} alt="" referrerPolicy="no-referrer" />
      )}
    </span>
  );
}

function Workspace({
  view,
  commandError,
  onDisconnectSteam,
  onDisconnectHumble,
  onRefreshEntitlements,
  onCancelEntitlements,
  onLoadSteamReviews,
  onSetMapping,
  onClearMapping,
  onSearchSteam,
  onLoadGameDetails,
  onOpenSteam,
  onOpenHumble,
}: {
  view: AppView;
  commandError: string | null;
  onDisconnectSteam: () => Promise<void>;
  onDisconnectHumble: () => Promise<void>;
  onRefreshEntitlements: () => Promise<void>;
  onCancelEntitlements: () => Promise<void>;
  onLoadSteamReviews: () => Promise<void>;
  onSetMapping: (
    mappingKey: string,
    appId: number,
    steamName: string,
  ) => Promise<void>;
  onClearMapping: (mappingKey: string) => Promise<void>;
  onSearchSteam: (query: string) => Promise<MappingCandidate[]>;
  onLoadGameDetails: (appId: number) => Promise<SteamGameDetails>;
  onOpenSteam: (appId: number) => Promise<void>;
  onOpenHumble: (url: string) => Promise<void>;
}) {
  const [activeSection, setActiveSection] = useState<
    "matches" | "entitlements" | "people" | "mappings"
  >("matches");
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<"available" | "review" | "all">(
    "available",
  );
  const [showResolvedMappings, setShowResolvedMappings] = useState(false);
  const [selectedEntitlementId, setSelectedEntitlementId] = useState<
    string | null
  >(null);
  const [humbleBrowserOpening, setHumbleBrowserOpening] = useState(false);
  const humbleBrowserOpeningRef = useRef(false);
  const selectedEntitlement =
    view.entitlements.items.find(
      (item) => item.id === selectedEntitlementId,
    ) ?? null;
  const inspectEntitlement = (item: Entitlement) => {
    if (item.steamAppId && item.status !== "needs_mapping") {
      setSelectedEntitlementId(item.id);
    }
  };
  const openHumblePage = useCallback(
    async (url: string) => {
      if (humbleBrowserOpeningRef.current) return;
      humbleBrowserOpeningRef.current = true;
      setHumbleBrowserOpening(true);
      try {
        await onOpenHumble(url);
      } finally {
        humbleBrowserOpeningRef.current = false;
        setHumbleBrowserOpening(false);
      }
    },
    [onOpenHumble],
  );
  const openEntitlementSource = (item: Entitlement) => {
    if (item.purchaseUrl) {
      void openHumblePage(item.purchaseUrl);
    }
  };
  const normalizedQuery = query.trim().toLowerCase();
  const matchesQuery = (values: Array<string | number | null | undefined>) =>
    !normalizedQuery ||
    values.some((value) =>
      String(value ?? "")
        .toLowerCase()
        .includes(normalizedQuery),
    );
  const entitlementItems = view.entitlements.items.filter((item) => {
    const inScope =
      scope === "all" ||
      (scope === "available" &&
        ["available", "needs_mapping"].includes(item.status)) ||
      (scope === "review" &&
        ["needs_mapping", "expired", "hidden"].includes(item.status));
    if (!inScope) return false;
    return matchesQuery([
      item.name,
      item.parentName,
      item.steamName,
      item.steamAppId,
    ]);
  });
  const mappingItems = view.entitlements.items.filter(
    (item) =>
      (item.status === "needs_mapping" ||
        (showResolvedMappings &&
          (item.mappingSource === "automatic" ||
            item.mappingSource === "manual"))) &&
      matchesQuery([
        item.name,
        item.parentName,
        item.steamName,
        item.steamAppId,
      ]),
  );
  const people = view.wishlists.people.filter((person) =>
    matchesQuery([person.displayName]),
  );
  const sectionHeading = {
    matches: {
      kicker: "Gift overview",
      title: "Wishlist matches",
      search: "Search games, bundles, or people",
    },
    entitlements: {
      kicker: "Humble library",
      title: "Browse entitlements",
      search: "Search entitlements or bundles",
    },
    people: {
      kicker: "Steam friends",
      title: "People and wishlist access",
      search: "Search people",
    },
    mappings: {
      kicker: "Match corrections",
      title: "Steam mappings",
      search: "Search uncertain games or AppIDs",
    },
  }[activeSection];
  const selectSection = (section: typeof activeSection) => {
    setActiveSection(section);
    setQuery("");
  };
  useEffect(() => {
    if (
      activeSection === "entitlements" &&
      view.entitlements.phase === "complete" &&
      view.steamReviews.phase === "idle"
    ) {
      void onLoadSteamReviews();
    }
  }, [
    activeSection,
    onLoadSteamReviews,
    view.entitlements.phase,
    view.steamReviews.phase,
  ]);

  return (
    <main className="product-shell">
      <aside className="sidebar">
        <Brand />
        <nav aria-label="Main navigation">
          <button
            type="button"
            className={`nav-item${activeSection === "matches" ? " active" : ""}`}
            aria-current={activeSection === "matches" ? "page" : undefined}
            onClick={() => selectSection("matches")}
          >
            <GridIcon />
            <span>
              <strong>Matches</strong>
              <small>Your available gifts</small>
            </span>
          </button>
          <button
            type="button"
            className={`nav-item${activeSection === "entitlements" ? " active" : ""}`}
            aria-current={
              activeSection === "entitlements" ? "page" : undefined
            }
            onClick={() => selectSection("entitlements")}
          >
            <LibraryIcon />
            <span>
              <strong>Entitlements</strong>
              <small>Discover forgotten games</small>
            </span>
          </button>
          <button
            type="button"
            className={`nav-item${activeSection === "people" ? " active" : ""}`}
            aria-current={activeSection === "people" ? "page" : undefined}
            onClick={() => selectSection("people")}
          >
            <PeopleIcon />
            <span>
              <strong>People</strong>
              <small>Wishlist access</small>
            </span>
          </button>
          <button
            type="button"
            className={`nav-item${activeSection === "mappings" ? " active" : ""}`}
            aria-current={activeSection === "mappings" ? "page" : undefined}
            onClick={() => selectSection("mappings")}
          >
            <LinkIcon />
            <span>
              <strong>Mappings</strong>
              <small>Resolve uncertain games</small>
            </span>
          </button>
        </nav>

        <div className="sidebar-connections">
          <p>Connections</p>
          <ConnectionRow
            label={view.steam.profile?.displayName ?? "Steam"}
            detail="Steam"
            avatar={view.steam.profile}
            onDisconnect={onDisconnectSteam}
          />
          <ConnectionRow
            label="Humble Bundle"
            detail="Library"
            onDisconnect={onDisconnectHumble}
          />
        </div>
      </aside>

      <section className="main-pane">
        <header className="app-header">
          <div>
            <p className="kicker">{sectionHeading.kicker}</p>
            <h1>{sectionHeading.title}</h1>
          </div>
          <label className="global-search">
            <SearchIcon />
            <input
              aria-label={`Search ${activeSection}`}
              placeholder={sectionHeading.search}
              value={query}
              onChange={(event) => setQuery(event.currentTarget.value)}
            />
            <kbd>⌘ K</kbd>
          </label>
          <span className="sync-state">
            <i />
            Accounts connected
          </span>
        </header>

        {view.developmentCache && (
          <aside className="development-banner">
            <span>Insecure development cache</span>
            Sessions are currently stored in the local development cache.
          </aside>
        )}

        <section className="summary-grid" aria-label="Match summary">
          <SummaryCard
            label="Available gifts"
            value={
              view.entitlements.phase === "complete"
                ? view.entitlements.summary.available.toLocaleString()
                : "—"
            }
            detail={
              view.entitlements.phase === "complete"
                ? `${view.entitlements.summary.needsMapping} need mapping`
                : "Loading from Humble"
            }
            tone="blue"
          />
          <SummaryCard
            label="Wishlist matches"
            value={
              view.wishlists.phase === "complete"
                ? view.wishlists.matches.length.toLocaleString()
                : "—"
            }
            detail={
              view.wishlists.phase === "complete"
                ? `${view.wishlists.wishlistApps.toLocaleString()} distinct wished-for games`
                : view.wishlists.message
            }
            tone="green"
          />
          <SummaryCard
            label="People checked"
            value={
              view.wishlists.phase === "complete"
                ? view.wishlists.peopleTotal.toLocaleString()
                : "—"
            }
            detail={
              view.wishlists.phase === "complete"
                ? `${view.wishlists.peopleAccessible} accessible · ${view.wishlists.peopleInaccessible} private`
                : "Loading Steam friends"
            }
            tone="violet"
          />
        </section>

        {activeSection === "matches" && (
          <>
            <MatchWorkspace
              view={view}
              query={query}
              onInspectEntitlement={inspectEntitlement}
              onOpenHumble={openEntitlementSource}
            />
            <EntitlementWorkspace
              view={view}
              scope={scope}
              items={entitlementItems}
              onScopeChange={setScope}
              onRefresh={onRefreshEntitlements}
              onCancel={onCancelEntitlements}
              onSetMapping={onSetMapping}
              onClearMapping={onClearMapping}
              onSearchSteam={onSearchSteam}
              onInspectEntitlement={inspectEntitlement}
              onOpenHumble={openEntitlementSource}
            />
          </>
        )}

        {activeSection === "people" && (
          <PeopleWorkspace
            view={view}
            people={people}
            query={query}
            onInspectEntitlement={inspectEntitlement}
          />
        )}

        {activeSection === "entitlements" && (
          <EntitlementDiscoveryWorkspace
            view={view}
            query={query}
            onInspectEntitlement={inspectEntitlement}
            onOpenHumble={openEntitlementSource}
          />
        )}

        {activeSection === "mappings" && (
          <EntitlementWorkspace
            view={view}
            scope="review"
            items={mappingItems}
            onScopeChange={setScope}
            onRefresh={onRefreshEntitlements}
            onCancel={onCancelEntitlements}
            onSetMapping={onSetMapping}
            onClearMapping={onClearMapping}
            onSearchSteam={onSearchSteam}
            onInspectEntitlement={inspectEntitlement}
            onOpenHumble={openEntitlementSource}
            showScope={false}
            showResolvedMappings={showResolvedMappings}
            onShowResolvedMappingsChange={setShowResolvedMappings}
            kicker="Local corrections"
            title="Steam mapping corrections"
            subtitle={
              showResolvedMappings
                ? `${view.entitlements.summary.needsMapping.toLocaleString()} unresolved · ${mappingItems.filter((item) => item.mappingSource === "automatic").length.toLocaleString()} automatic · ${mappingItems.filter((item) => item.mappingSource === "manual").length.toLocaleString()} corrected`
                : `${view.entitlements.summary.needsMapping.toLocaleString()} need action`
            }
            emptyTitle="No mappings need attention"
            emptyMessage={
              query
                ? "No mapping records match this search."
                : "Uncertain games will appear here with local Steam suggestions."
            }
          />
        )}

        {commandError && <p className="command-error">{commandError}</p>}
      </section>
      {selectedEntitlement && selectedEntitlement.steamAppId && (
        <GameDetailsDrawer
          entitlement={selectedEntitlement}
          onClose={() => setSelectedEntitlementId(null)}
          onLoad={onLoadGameDetails}
          onOpenSteam={onOpenSteam}
          onOpenHumble={openHumblePage}
        />
      )}
      {humbleBrowserOpening && (
        <aside
          className="humble-browser-loading"
          role="status"
          aria-label="Opening Humble"
          aria-live="polite"
        >
          <span className="match-spinner" aria-hidden="true" />
          <span>
            <strong>Opening Humble…</strong>
            <small>Preparing your signed-in browser</small>
          </span>
        </aside>
      )}
    </main>
  );
}

function OwnedBadge() {
  return (
    <span
      className="owned-badge"
      data-tooltip="Already in your Steam library"
      aria-label="Already in your Steam library"
      role="img"
      tabIndex={0}
    >
      ✓
    </span>
  );
}

function ExpiryBadge({ item }: { item: Entitlement }) {
  if (!item.expirationDate) return null;
  const label = `Key ${
    item.status === "expired" ? "expired" : "expires"
  } ${formatExpirationDate(item.expirationDate)}`;
  return (
    <span
      className="expiry-badge"
      data-tooltip={label}
      aria-label={label}
      role="img"
      tabIndex={0}
    >
      ⌛
    </span>
  );
}

function formatExpirationDate(value: string) {
  const [year, month, day] = value
    .slice(0, 10)
    .split("-")
    .map(Number);
  return new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(new Date(year, month - 1, day));
}

function comparableGameTitle(title: string) {
  return title
    .normalize("NFKD")
    .toLocaleLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

function humbleTitleDiffers(item: Entitlement) {
  return (
    Boolean(item.steamName) &&
    comparableGameTitle(item.name) !== comparableGameTitle(item.steamName!)
  );
}

function MatchWorkspace({
  view,
  query,
  onInspectEntitlement,
  onOpenHumble,
}: {
  view: AppView;
  query: string;
  onInspectEntitlement: (item: Entitlement) => void;
  onOpenHumble: (item: Entitlement) => void;
}) {
  const sync = view.wishlists;
  const ownedAppIds = new Set(view.steam.ownedAppIds);
  const normalizedQuery = query.trim().toLowerCase();
  const matches = sync.matches.filter(
    (match) =>
      !normalizedQuery ||
      match.steamName.toLowerCase().includes(normalizedQuery) ||
      String(match.appId).includes(normalizedQuery) ||
      match.wishers.some((person) =>
        person.displayName.toLowerCase().includes(normalizedQuery),
      ),
  );
  if (sync.phase === "idle") return null;
  if (sync.phase === "loading") {
    return (
      <section className="match-panel match-loading" aria-live="polite">
        <span className="match-spinner" aria-hidden="true" />
        <span>
          <strong>Matching Steam wishlists</strong>
          <small>{sync.message}</small>
        </span>
      </section>
    );
  }
  if (sync.phase === "error") {
    return (
      <section className="match-panel match-error" aria-live="polite">
        <strong>Wishlist matching needs attention</strong>
        <small>{sync.error ?? sync.message}</small>
      </section>
    );
  }
  return (
    <section className="match-workspace">
      <header>
        <span>
          <p className="kicker">Ready to gift</p>
          <h2>Games friends want</h2>
        </span>
        <small>{sync.message}</small>
      </header>
      {matches.length ? (
        <div className="match-grid">
          {matches.map((match) => {
            const entitlement = view.entitlements.items.find(
              (item) => item.id === match.entitlementId,
            );
            return (
              <article className="match-card" key={match.entitlementId}>
                <button
                  type="button"
                  className="game-art-button"
                  aria-label={`View details for ${match.steamName}`}
                  disabled={!entitlement}
                  onClick={() =>
                    entitlement && onInspectEntitlement(entitlement)
                  }
                >
                  <i
                    className={`match-art art-${match.appId % 6}`}
                    aria-hidden="true"
                  >
                    <span>{match.steamName.slice(0, 2).toUpperCase()}</span>
                    <img
                      src={steamArtworkUrl(match.appId)}
                      alt=""
                      loading="lazy"
                      referrerPolicy="no-referrer"
                      onError={(event) => event.currentTarget.remove()}
                    />
                  </i>
                </button>
                <span className="match-copy">
                  <span className="game-title-line">
                    <button
                      type="button"
                      className="game-title-button"
                      disabled={!entitlement}
                      onClick={() =>
                        entitlement && onInspectEntitlement(entitlement)
                      }
                    >
                      {match.steamName}
                    </button>
                    {ownedAppIds.has(match.appId) && <OwnedBadge />}
                    {entitlement && <ExpiryBadge item={entitlement} />}
                  </span>
                  {entitlement?.purchaseUrl ? (
                    <button
                      type="button"
                      className="source-link"
                      onClick={() => onOpenHumble(entitlement)}
                    >
                      {entitlement.parentName}
                    </button>
                  ) : (
                    <small>{entitlement?.parentName ?? `App ${match.appId}`}</small>
                  )}
                </span>
                <span className="wisher-stack">
                  {match.wishers.slice(0, 4).map((person) =>
                    person.avatarUrl ? (
                      <img
                        src={person.avatarUrl}
                        alt={person.displayName}
                        title={person.displayName}
                        loading="lazy"
                        key={person.steamId}
                      />
                    ) : (
                      <i title={person.displayName} key={person.steamId}>
                        {person.displayName.slice(0, 1).toUpperCase()}
                      </i>
                    ),
                  )}
                  <small>
                    {match.wishers.length === 1
                      ? match.wishers[0].isSelf
                        ? "On your wishlist"
                        : match.wishers[0].displayName
                      : `${match.wishers.length} people`}
                  </small>
                </span>
              </article>
            );
          })}
        </div>
      ) : (
        <div className="match-empty">
          <strong>No wishlist matches yet</strong>
          <small>
            {query
              ? "No wishlist matches fit this search."
              : sync.peopleAccessible
              ? "None of the available entitlements appeared on an accessible wishlist."
              : "No accessible Steam wishlists were found."}
          </small>
        </div>
      )}
      {sync.error && <p className="match-warning">{sync.error}</p>}
    </section>
  );
}

function EntitlementDiscoveryWorkspace({
  view,
  query,
  onInspectEntitlement,
  onOpenHumble,
}: {
  view: AppView;
  query: string;
  onInspectEntitlement: (item: Entitlement) => void;
  onOpenHumble: (item: Entitlement) => void;
}) {
  const [sort, setSort] = useState<"rating" | "reviews">("rating");
  const [expiringOnly, setExpiringOnly] = useState(false);
  const ownedAppIds = new Set(view.steam.ownedAppIds);
  const normalizedQuery = query.trim().toLowerCase();
  const reviewFor = (appId: number | null) =>
    appId ? view.steamReviews.items[appId] : undefined;
  const items = view.entitlements.items
    .filter(
      (item) =>
        item.status === "available" &&
        item.steamAppId &&
        (!expiringOnly || item.expirationDate) &&
        (!normalizedQuery ||
          [item.name, item.steamName, item.parentName, item.steamAppId].some(
            (value) =>
              String(value ?? "")
                .toLowerCase()
                .includes(normalizedQuery),
          )),
    )
    .sort((left, right) => {
      if (expiringOnly) {
        const expiryOrder = (left.expirationDate ?? "").localeCompare(
          right.expirationDate ?? "",
        );
        if (expiryOrder !== 0) return expiryOrder;
      }
      const leftReview = reviewFor(left.steamAppId);
      const rightReview = reviewFor(right.steamAppId);
      const leftRating = leftReview?.positivePercentage ?? -1;
      const rightRating = rightReview?.positivePercentage ?? -1;
      const leftReviews = leftReview?.totalReviews ?? -1;
      const rightReviews = rightReview?.totalReviews ?? -1;
      const primary =
        sort === "rating"
          ? rightRating - leftRating
          : rightReviews - leftReviews;
      if (primary !== 0) return primary;
      const secondary =
        sort === "rating"
          ? rightReviews - leftReviews
          : rightRating - leftRating;
      if (secondary !== 0) return secondary;
      return left.name.localeCompare(right.name);
    });
  const reviewSync = view.steamReviews;
  const progress =
    reviewSync.total > 0
      ? Math.round((reviewSync.completed / reviewSync.total) * 100)
      : 0;

  return (
    <section className="discovery-workspace">
      <header className="discovery-toolbar">
        <div>
          <p className="kicker">Unrevealed on Humble</p>
          <h2>Hidden gems in your Humble library</h2>
          <small>
            {expiringOnly
              ? `${items.length.toLocaleString()} expiring entitlements · soonest first`
              : `${items.length.toLocaleString()} mapped Steam entitlements · lifetime user reviews`}
          </small>
        </div>
        <div className="discovery-controls">
          {!expiringOnly && (
            <div className="sort-control" aria-label="Entitlement sorting">
              <button
                type="button"
                className={sort === "rating" ? "active" : ""}
                aria-pressed={sort === "rating"}
                onClick={() => setSort("rating")}
              >
                Top rated
              </button>
              <button
                type="button"
                className={sort === "reviews" ? "active" : ""}
                aria-pressed={sort === "reviews"}
                onClick={() => setSort("reviews")}
              >
                Most reviewed
              </button>
            </div>
          )}
          <button
            type="button"
            className={`expiry-filter${expiringOnly ? " active" : ""}`}
            aria-pressed={expiringOnly}
            onClick={() => setExpiringOnly((current) => !current)}
          >
            <span aria-hidden="true">⌛</span>
            Expiring only
          </button>
        </div>
      </header>

      {reviewSync.phase === "loading" && (
        <div className="review-loading" aria-live="polite">
          <span>
            <i style={{ width: `${progress}%` }} />
          </span>
          <small>{reviewSync.message}</small>
        </div>
      )}

      {items.length ? (
        <div
          className={`discovery-table${expiringOnly ? " expiring" : ""}`}
          role="table"
          aria-label={
            expiringOnly
              ? "Available Humble entitlements sorted by expiry date"
              : "Available Humble entitlements ranked by Steam reviews"
          }
        >
          <div className="discovery-table-head" role="row">
            <span role="columnheader">Game</span>
            <span role="columnheader">Humble collection</span>
            {expiringOnly && <span role="columnheader">Expiry</span>}
            <span role="columnheader">Steam rating</span>
          </div>
          {items.map((item) => {
            const review = reviewFor(item.steamAppId);
            return (
              <article className="discovery-row" role="row" key={item.id}>
                <div className="entitlement-name" role="cell">
                  <button
                    type="button"
                    className="game-art-button"
                    aria-label={`View details for ${item.steamName ?? item.name}`}
                    onClick={() => onInspectEntitlement(item)}
                  >
                    <EntitlementArtwork item={item} />
                  </button>
                  <span>
                    <span className="game-title-line">
                      <button
                        type="button"
                        className="game-title-button"
                        onClick={() => onInspectEntitlement(item)}
                      >
                        {item.steamName ?? item.name}
                      </button>
                      {item.steamAppId &&
                        ownedAppIds.has(item.steamAppId) && <OwnedBadge />}
                      <ExpiryBadge item={item} />
                    </span>
                    <small>Steam App {item.steamAppId}</small>
                  </span>
                </div>
                <div className="discovery-source" role="cell">
                  {item.purchaseUrl ? (
                    <button
                      type="button"
                      className="source-link"
                      onClick={() => onOpenHumble(item)}
                    >
                      {item.parentName}
                    </button>
                  ) : (
                    <strong>{item.parentName}</strong>
                  )}
                  {humbleTitleDiffers(item) && (
                    <small>Humble title: {item.name}</small>
                  )}
                </div>
                {expiringOnly && item.expirationDate && (
                  <div className="discovery-expiry" role="cell">
                    <span aria-hidden="true">⌛</span>
                    <time dateTime={item.expirationDate}>
                      {formatExpirationDate(item.expirationDate)}
                    </time>
                  </div>
                )}
                <div className="review-score" role="cell">
                  {review?.positivePercentage !== null &&
                  review?.positivePercentage !== undefined ? (
                    <>
                      <span>
                        <strong>
                          {Math.round(review.positivePercentage)}% positive
                        </strong>
                        <small>{review.scoreDescription}</small>
                      </span>
                      <span className="review-meter" aria-hidden="true">
                        <i
                          style={{
                            width: `${Math.round(review.positivePercentage)}%`,
                          }}
                        />
                      </span>
                      <small>
                        {review.totalReviews.toLocaleString()} reviews
                      </small>
                    </>
                  ) : (
                    <>
                      <strong className="review-pending">
                        {reviewSync.phase === "loading"
                          ? "Loading…"
                          : "Not rated"}
                      </strong>
                      <small>
                        {review?.scoreDescription ??
                          "Steam review data unavailable"}
                      </small>
                    </>
                  )}
                </div>
              </article>
            );
          })}
        </div>
      ) : (
        <div className="filtered-empty">
          <span aria-hidden="true">◇</span>
          <strong>No available entitlements found</strong>
          <small>
            {expiringOnly
              ? "No available Steam gifts with expiry dates match this view."
              : query
              ? "No available Steam gifts match this search."
              : "Available, mapped Steam entitlements will appear here."}
          </small>
        </div>
      )}
      {reviewSync.error && <p className="match-warning">{reviewSync.error}</p>}
    </section>
  );
}

function PeopleWorkspace({
  view,
  people,
  query,
  onInspectEntitlement,
}: {
  view: AppView;
  people: AppView["wishlists"]["people"];
  query: string;
  onInspectEntitlement: (item: Entitlement) => void;
}) {
  const [selectedSteamId, setSelectedSteamId] = useState<string | null>(null);
  const sync = view.wishlists;
  const selectedPerson = selectedSteamId
    ? view.wishlists.people.find(
        (person) => person.steamId === selectedSteamId,
      ) ?? null
    : null;
  const selectedWishlist = selectedPerson
    ? view.wishlists.wishlistItems[selectedPerson.steamId] ?? []
    : [];
  if (sync.phase === "idle" || sync.phase === "loading") {
    return (
      <section className="match-panel people-loading" aria-live="polite">
        <span className="match-spinner" aria-hidden="true" />
        <span>
          <strong>Checking Steam wishlist access</strong>
          <small>{sync.message}</small>
        </span>
      </section>
    );
  }

  return (
    <section className="people-workspace">
      <header>
        <span>
          <p className="kicker">Wishlist coverage</p>
          <h2>People checked</h2>
        </span>
        <small>
          {sync.peopleAccessible} accessible · {sync.peopleInaccessible} private
          or unavailable
        </small>
      </header>
      {selectedPerson ? (
        <div className="wishlist-detail">
          <div className="wishlist-detail-header">
            <button
              type="button"
              className="wishlist-back"
              onClick={() => setSelectedSteamId(null)}
            >
              <span aria-hidden="true">←</span>
              All people
            </button>
            <span className="person-avatar" aria-hidden="true">
              {selectedPerson.displayName.slice(0, 1).toUpperCase()}
              {selectedPerson.avatarUrl && (
                <img
                  src={selectedPerson.avatarUrl}
                  alt=""
                  referrerPolicy="no-referrer"
                />
              )}
            </span>
            <span>
              <strong>{selectedPerson.displayName}</strong>
              <small>
                {selectedWishlist.length.toLocaleString()} wished-for games
              </small>
            </span>
          </div>
          {selectedWishlist.length ? (
            <div className="wishlist-game-grid">
              {selectedWishlist.map((game) => {
                const entitlement = view.entitlements.items.find(
                  (item) =>
                    item.status === "available" &&
                    item.steamAppId === game.appId,
                );
                return (
                  <article className="wishlist-game-card" key={game.appId}>
                    {entitlement ? (
                      <button
                        type="button"
                        className="game-art-button"
                        aria-label={`View entitlement details for ${game.name}`}
                        onClick={() => onInspectEntitlement(entitlement)}
                      >
                        <i aria-hidden="true">
                          <b>{game.name.slice(0, 2).toUpperCase()}</b>
                          <img
                            src={steamArtworkUrl(game.appId)}
                            alt=""
                            loading="lazy"
                            referrerPolicy="no-referrer"
                          />
                        </i>
                      </button>
                    ) : (
                      <i aria-hidden="true">
                        <b>{game.name.slice(0, 2).toUpperCase()}</b>
                        <img
                          src={steamArtworkUrl(game.appId)}
                          alt=""
                          loading="lazy"
                          referrerPolicy="no-referrer"
                        />
                      </i>
                    )}
                    <span>
                      {entitlement ? (
                        <button
                          type="button"
                          className="game-title-button"
                          onClick={() => onInspectEntitlement(entitlement)}
                        >
                          {game.name}
                        </button>
                      ) : (
                        <strong>{game.name}</strong>
                      )}
                      <small>Steam App {game.appId}</small>
                    </span>
                  </article>
                );
              })}
            </div>
          ) : (
            <div className="filtered-empty">
              <span aria-hidden="true">◇</span>
              <strong>This wishlist is empty</strong>
              <small>Steam did not return any wished-for games.</small>
            </div>
          )}
        </div>
      ) : people.length ? (
        <div className="people-grid">
          {people.map((person) => (
            <button
              type="button"
              className="person-card"
              key={person.steamId}
              disabled={person.wishlistAccess !== "accessible"}
              onClick={() => setSelectedSteamId(person.steamId)}
            >
              <span className="person-avatar" aria-hidden="true">
                {person.displayName.slice(0, 1).toUpperCase()}
                {person.avatarUrl && (
                  <img
                    src={person.avatarUrl}
                    alt=""
                    loading="lazy"
                    referrerPolicy="no-referrer"
                  />
                )}
              </span>
              <span className="person-copy">
                <strong>
                  {person.displayName}
                  {person.isSelf && <small>You</small>}
                </strong>
                <span
                  className={`wishlist-access ${person.wishlistAccess}`}
                >
                  {person.wishlistAccess === "accessible"
                    ? `${person.wishlistCount.toLocaleString()} wished-for games`
                    : person.wishlistAccess === "inaccessible"
                      ? "Private wishlist"
                      : "Could not be checked"}
                  </span>
              </span>
              {person.wishlistAccess === "accessible" && (
                <span className="person-card-arrow" aria-hidden="true">
                  ›
                </span>
              )}
            </button>
          ))}
        </div>
      ) : (
        <div className="filtered-empty">
          <span aria-hidden="true">◇</span>
          <strong>No people found</strong>
          <small>
            {query
              ? "No Steam friends match this search."
              : "Steam did not return any friends."}
          </small>
        </div>
      )}
      {sync.error && <p className="match-warning">{sync.error}</p>}
    </section>
  );
}

function EntitlementWorkspace({
  view,
  scope,
  items,
  onScopeChange,
  onRefresh,
  onCancel,
  onSetMapping,
  onClearMapping,
  onSearchSteam,
  onInspectEntitlement,
  onOpenHumble,
  showScope = true,
  showResolvedMappings = false,
  onShowResolvedMappingsChange,
  kicker = "Humble library",
  title = "Gift-ready entitlements",
  subtitle,
  emptyTitle = "No entitlements in this view",
  emptyMessage = "Try another scope or search.",
}: {
  view: AppView;
  scope: "available" | "review" | "all";
  items: AppView["entitlements"]["items"];
  onScopeChange: (scope: "available" | "review" | "all") => void;
  onRefresh: () => Promise<void>;
  onCancel: () => Promise<void>;
  onSetMapping: (
    mappingKey: string,
    appId: number,
    steamName: string,
  ) => Promise<void>;
  onClearMapping: (mappingKey: string) => Promise<void>;
  onSearchSteam: (query: string) => Promise<MappingCandidate[]>;
  onInspectEntitlement: (item: Entitlement) => void;
  onOpenHumble: (item: Entitlement) => void;
  showScope?: boolean;
  showResolvedMappings?: boolean;
  onShowResolvedMappingsChange?: (show: boolean) => void;
  kicker?: string;
  title?: string;
  subtitle?: string;
  emptyTitle?: string;
  emptyMessage?: string;
}) {
  const [editingMapping, setEditingMapping] = useState<string | null>(null);
  const sync = view.entitlements;
  const ownedAppIds = new Set(view.steam.ownedAppIds);
  if (sync.phase === "idle" || sync.phase === "loading") {
    const progress =
      sync.totalOrders > 0
        ? (sync.completedOrders / sync.totalOrders) * 100
        : 8;
    return (
      <section className="sync-workspace" aria-live="polite">
        <span className="empty-illustration loading" aria-hidden="true">
          <GiftMark />
          <i />
          <b />
        </span>
        <p className="kicker">Reading Humble</p>
        <h2>Loading your gift library.</h2>
        <p>{sync.message}</p>
        <div className="sync-progress">
          <span style={{ width: `${Math.max(progress, 8)}%` }} />
        </div>
        {sync.totalOrders > 0 && (
          <small>
            {sync.completedOrders.toLocaleString()} of{" "}
            {sync.totalOrders.toLocaleString()} purchases
          </small>
        )}
        {sync.phase === "loading" ? (
          <button className="quiet-button" onClick={onCancel}>
            Cancel
          </button>
        ) : (
          <button className="primary-button" onClick={onRefresh}>
            Load gifts
          </button>
        )}
      </section>
    );
  }

  if (sync.phase === "error") {
    return (
      <section className="sync-workspace error-workspace" aria-live="polite">
        <span className="error-mark" aria-hidden="true">
          !
        </span>
        <p className="kicker">Humble needs attention</p>
        <h2>We couldn&apos;t load your library.</h2>
        <p>{sync.error ?? sync.message}</p>
        <button className="primary-button" onClick={onRefresh}>
          Try again
        </button>
      </section>
    );
  }

  return (
    <section className="entitlement-workspace">
      <header className="entitlement-toolbar">
        <div>
          <p className="kicker">{kicker}</p>
          <h2>{title}</h2>
          <small>
            {subtitle ??
              `${sync.summary.total.toLocaleString()} entitlements classified${
                sync.refreshedAt
                  ? ` · refreshed ${formatRefreshedAt(sync.refreshedAt)}`
                  : ""
              }`}
          </small>
        </div>
        <div className="toolbar-actions">
          {onShowResolvedMappingsChange && (
            <label className="resolved-mappings-toggle">
              <input
                type="checkbox"
                checked={showResolvedMappings}
                onChange={(event) =>
                  onShowResolvedMappingsChange(event.currentTarget.checked)
                }
              />
              <span aria-hidden="true">
                <i />
              </span>
              Show matched
            </label>
          )}
          {showScope && (
            <div className="scope-control" aria-label="Entitlement scope">
              {(["available", "review", "all"] as const).map((option) => (
                <button
                  className={scope === option ? "active" : ""}
                  key={option}
                  onClick={() => onScopeChange(option)}
                >
                  {option === "available"
                    ? "Available"
                    : option === "review"
                      ? "Needs review"
                      : "All"}
                </button>
              ))}
            </div>
          )}
          <button
            className="quiet-button refresh-button"
            onClick={onRefresh}
            aria-label="Refresh Humble"
            title="Refresh Humble"
          >
            <span aria-hidden="true">↻</span>
          </button>
        </div>
      </header>

      {items.length ? (
        <div className="entitlement-table" role="table" aria-label="Humble entitlements">
            <div className="entitlement-table-head" role="row">
              <span role="columnheader">Game</span>
              <span role="columnheader">Steam mapping</span>
              <span role="columnheader">Status</span>
            </div>
            {items.map((item) => (
              <article className="entitlement-row" role="row" key={item.id}>
                <div className="entitlement-name" role="cell">
                  {item.steamAppId && item.status !== "needs_mapping" ? (
                    <button
                      type="button"
                      className="game-art-button"
                      aria-label={`View details for ${item.steamName ?? item.name}`}
                      onClick={() => onInspectEntitlement(item)}
                    >
                      <EntitlementArtwork item={item} />
                    </button>
                  ) : (
                    <EntitlementArtwork item={item} />
                  )}
                  <span>
                    {item.steamAppId && item.status !== "needs_mapping" ? (
                      <span className="game-title-line">
                        <button
                          type="button"
                          className="game-title-button"
                          onClick={() => onInspectEntitlement(item)}
                        >
                          {item.name}
                        </button>
                        {ownedAppIds.has(item.steamAppId) && <OwnedBadge />}
                        <ExpiryBadge item={item} />
                      </span>
                    ) : (
                      <span className="game-title-line">
                        <strong>{item.name}</strong>
                        <ExpiryBadge item={item} />
                      </span>
                    )}
                    {item.purchaseUrl ? (
                      <button
                        type="button"
                        className="source-link"
                        onClick={() => onOpenHumble(item)}
                      >
                        {item.parentName}
                      </button>
                    ) : (
                      <small>{item.parentName}</small>
                    )}
                  </span>
                </div>
                <div className="mapping-cell" role="cell">
                  <strong>
                    {item.steamName ??
                      (item.steamAppId ? `App ${item.steamAppId}` : "No AppID")}
                  </strong>
                  <small>
                    {item.mappingSource === "automatic"
                      ? `Auto-matched · App ${item.steamAppId}`
                      : item.mappingSource === "manual"
                        ? `Corrected locally · App ${item.steamAppId}`
                        : item.packageAmbiguity
                      ? "Edition/package may differ"
                        : item.steamAppId
                          ? `App ${item.steamAppId}`
                          : item.keyTypeLabel}
                  </small>
                  {item.steamAppId && item.mappingSource !== "humble" && (
                    <button
                      className="mapping-button"
                      onClick={() =>
                        setEditingMapping((current) =>
                          current === item.mappingKey ? null : item.mappingKey,
                        )
                      }
                    >
                      {editingMapping === item.mappingKey ? "Cancel" : "Change"}
                    </button>
                  )}
                </div>
                <div className="status-cell" role="cell">
                  {item.status === "needs_mapping" ? (
                    <button
                      type="button"
                      className="entitlement-status needs_mapping status-button"
                      aria-expanded={editingMapping === item.mappingKey}
                      onClick={() =>
                        setEditingMapping((current) =>
                          current === item.mappingKey ? null : item.mappingKey,
                        )
                      }
                    >
                      {entitlementStatusLabel(item.status)}
                    </button>
                  ) : (
                    <span className={`entitlement-status ${item.status}`}>
                      {entitlementStatusLabel(item.status)}
                    </span>
                  )}
                  {(item.regionRestricted || item.packageAmbiguity) && (
                    <small>
                      {[
                        item.regionRestricted ? "Region restricted" : null,
                        item.packageAmbiguity ? "Package ambiguity" : null,
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </small>
                  )}
                </div>
                {editingMapping === item.mappingKey && (
                  <div className="mapping-editor-row" role="cell">
                    <MappingEditor
                      item={item}
                      onChoose={async (appId, steamName) => {
                        await onSetMapping(item.mappingKey, appId, steamName);
                        setEditingMapping(null);
                      }}
                      onClear={
                        item.steamAppId
                          ? async () => {
                              await onClearMapping(item.mappingKey);
                              setEditingMapping(null);
                            }
                          : undefined
                      }
                      onSearchSteam={onSearchSteam}
                    />
                  </div>
                )}
              </article>
            ))}
          </div>
      ) : (
        <div className="filtered-empty">
          <span aria-hidden="true">◇</span>
          <strong>{emptyTitle}</strong>
          <small>{emptyMessage}</small>
        </div>
      )}
    </section>
  );
}

function MappingEditor({
  item,
  onChoose,
  onClear,
  onSearchSteam,
}: {
  item: AppView["entitlements"]["items"][number];
  onChoose: (appId: number, steamName: string) => Promise<void>;
  onClear?: () => Promise<void>;
  onSearchSteam: (query: string) => Promise<MappingCandidate[]>;
}) {
  const [appId, setAppId] = useState("");
  const [searchTerm, setSearchTerm] = useState(item.name);
  const [searchResults, setSearchResults] = useState<MappingCandidate[] | null>(
    null,
  );
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  const search = async (event: FormEvent) => {
    event.preventDefault();
    if (searchTerm.trim().length < 2) return;
    setSearching(true);
    setSearchError(null);
    try {
      setSearchResults(await onSearchSteam(searchTerm.trim()));
    } catch (error) {
      setSearchError(String(error));
    } finally {
      setSearching(false);
    }
  };

  return (
    <div className="mapping-editor">
      {item.mappingCandidates.length > 0 && (
        <div className="mapping-suggestion-group">
          <small className="mapping-label">Likely wishlist matches</small>
          <div className="mapping-candidates">
            {item.mappingCandidates.map((candidate) => (
              <MappingCandidateButton
                candidate={candidate}
                onChoose={onChoose}
                key={candidate.appId}
              />
            ))}
          </div>
        </div>
      )}

      <form className="steam-search-form" onSubmit={search}>
        <label>
          <span>Search Steam by title</span>
          <input
            aria-label={`Search Steam for ${item.name}`}
            value={searchTerm}
            onChange={(event) => setSearchTerm(event.currentTarget.value)}
          />
          <small>Only this search text is sent to Steam.</small>
        </label>
        <button type="submit" disabled={searching}>
          {searching ? "Searching…" : "Search Steam"}
        </button>
      </form>

      {searchError && (
        <small className="mapping-search-error">{searchError}</small>
      )}

      {searchResults && (
        <div className="mapping-suggestion-group search-results">
          <small className="mapping-label">
            {searchResults.length
              ? "Steam search results"
              : "No Steam games found. Try a shorter title."}
          </small>
          <div className="mapping-candidates">
            {searchResults.map((candidate) => (
              <MappingCandidateButton
                candidate={candidate}
                onChoose={onChoose}
                key={candidate.appId}
              />
            ))}
          </div>
        </div>
      )}

      <details className="appid-fallback">
        <summary>Use a Steam AppID instead</summary>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            const parsed = Number(appId);
            if (Number.isInteger(parsed) && parsed > 0) {
              void onChoose(parsed, `Steam App ${parsed}`);
            }
          }}
        >
          <input
            aria-label={`Steam AppID for ${item.name}`}
            inputMode="numeric"
            placeholder="Steam AppID"
            value={appId}
            onChange={(event) => setAppId(event.currentTarget.value)}
          />
          <button type="submit">Save</button>
          {onClear && (
            <button type="button" onClick={onClear}>
              Clear
            </button>
          )}
        </form>
      </details>
    </div>
  );
}

function MappingCandidateButton({
  candidate,
  onChoose,
}: {
  candidate: MappingCandidate;
  onChoose: (appId: number, steamName: string) => Promise<void>;
}) {
  return (
    <button
      type="button"
      onClick={() => onChoose(candidate.appId, candidate.name)}
    >
      <strong>{candidate.name}</strong>
      <small>
        App {candidate.appId}
        {candidate.similarity >= 0.5
          ? ` · ${Math.round(candidate.similarity * 100)}% title match`
          : ""}
      </small>
    </button>
  );
}

export function steamArtworkUrl(appId: number) {
  return `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/header.jpg`;
}

function EntitlementArtwork({
  item,
}: {
  item: AppView["entitlements"]["items"][number];
}) {
  const artIndex = item.steamAppId ? item.steamAppId % 6 : 0;
  return (
    <i className={`game-initial art-${artIndex}`} aria-hidden="true">
      <b>{item.name.slice(0, 2).toUpperCase()}</b>
      {item.steamAppId && (
        <img
          src={steamArtworkUrl(item.steamAppId)}
          alt=""
          loading="lazy"
          referrerPolicy="no-referrer"
          onError={(event) => event.currentTarget.remove()}
        />
      )}
    </i>
  );
}

function GameDetailsDrawer({
  entitlement,
  onClose,
  onLoad,
  onOpenSteam,
  onOpenHumble,
}: {
  entitlement: Entitlement;
  onClose: () => void;
  onLoad: (appId: number) => Promise<SteamGameDetails>;
  onOpenSteam: (appId: number) => Promise<void>;
  onOpenHumble: (url: string) => Promise<void>;
}) {
  const [details, setDetails] = useState<SteamGameDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeScreenshot, setActiveScreenshot] = useState(0);
  const appId = entitlement.steamAppId;

  useEffect(() => {
    if (!appId) return;
    let disposed = false;
    setDetails(null);
    setError(null);
    setActiveScreenshot(0);
    void onLoad(appId)
      .then((result) => {
        if (!disposed) setDetails(result);
      })
      .catch((loadError) => {
        if (!disposed) setError(String(loadError));
      });
    return () => {
      disposed = true;
    };
  }, [appId, onLoad]);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  const heroImage =
    details?.screenshots[activeScreenshot] ??
    details?.headerImage ??
    (appId ? steamArtworkUrl(appId) : null);
  const review = details?.review;
  const creatorLine = [
    details?.developers.length
      ? `By ${details.developers.join(", ")}`
      : null,
    details?.releaseDate,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div
      className="game-details-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <aside
        className="game-details-drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="game-details-title"
      >
        <header className="game-details-header">
          <span>
            <p className="kicker">Steam game details</p>
            <small>App {appId}</small>
          </span>
          <button
            type="button"
            className="drawer-close"
            aria-label="Close game details"
            onClick={onClose}
          >
            ×
          </button>
        </header>

        {details ? (
          <div className="game-details-content">
            {heroImage && (
              <img
                className="game-details-hero"
                src={heroImage}
                alt=""
                referrerPolicy="no-referrer"
              />
            )}
            {details.screenshots.length > 1 && (
              <div
                className="game-screenshot-strip"
                aria-label="Steam screenshots"
              >
                {details.screenshots.map((screenshot, index) => (
                  <button
                    type="button"
                    className={index === activeScreenshot ? "active" : ""}
                    aria-label={`Show screenshot ${index + 1}`}
                    aria-pressed={index === activeScreenshot}
                    onClick={() => setActiveScreenshot(index)}
                    key={screenshot}
                  >
                    <img
                      src={screenshot}
                      alt=""
                      loading="lazy"
                      referrerPolicy="no-referrer"
                    />
                  </button>
                ))}
              </div>
            )}

            <section className="game-details-title">
              <p className="kicker">{entitlement.parentName}</p>
              <h2 id="game-details-title">{details.name}</h2>
              {creatorLine && <small>{creatorLine}</small>}
            </section>

            {review && (
              <section className="drawer-review">
                <span>
                  <small>Lifetime Steam rating</small>
                  <strong>
                    {review.positivePercentage === null
                      ? "Not rated"
                      : `${Math.round(review.positivePercentage)}% positive`}
                  </strong>
                </span>
                <span>
                  <small>{review.scoreDescription}</small>
                  <strong>{review.totalReviews.toLocaleString()} reviews</strong>
                </span>
                {review.positivePercentage !== null && (
                  <i aria-hidden="true">
                    <b
                      style={{
                        width: `${Math.round(review.positivePercentage)}%`,
                      }}
                    />
                  </i>
                )}
              </section>
            )}

            {details.genres.length > 0 && (
              <section className="detail-group">
                <h3>Genres</h3>
                <div className="detail-tags">
                  {details.genres.map((genre) => (
                    <span key={genre}>{genre}</span>
                  ))}
                </div>
              </section>
            )}

            {details.summary && (
              <p className="game-summary">{details.summary}</p>
            )}
            {details.description &&
              details.description !== details.summary && (
                <section className="detail-group game-description">
                  <h3>About this game</h3>
                  <p>{details.description}</p>
                </section>
              )}

            {details.features.length > 0 && (
              <section className="detail-group">
                <h3>Features</h3>
                <div className="detail-tags muted">
                  {details.features.map((feature) => (
                    <span key={feature}>{feature}</span>
                  ))}
                </div>
              </section>
            )}
          </div>
        ) : error ? (
          <div className="drawer-state error">
            <strong>Steam details are unavailable</strong>
            <small>{error}</small>
          </div>
        ) : (
          <div className="drawer-state" aria-live="polite">
            <span className="match-spinner" aria-hidden="true" />
            <strong>Loading Steam details…</strong>
          </div>
        )}

        <footer className="game-details-actions">
          <button
            type="button"
            className="steam-link-button"
            onClick={() => appId && void onOpenSteam(appId)}
          >
            Open in Steam
            <span aria-hidden="true">↗</span>
          </button>
          {entitlement.purchaseUrl && (
            <button
              type="button"
              className="humble-link-button"
              onClick={() =>
                entitlement.purchaseUrl &&
                void onOpenHumble(entitlement.purchaseUrl)
              }
            >
              View on Humble
              <span aria-hidden="true">↗</span>
            </button>
          )}
        </footer>
      </aside>
    </div>
  );
}

function formatRefreshedAt(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp * 1000));
}

function entitlementStatusLabel(status: string) {
  switch (status) {
    case "available":
      return "Available";
    case "needs_mapping":
      return "Needs mapping";
    case "revealed":
      return "Revealed";
    case "expired":
      return "Expired";
    case "hidden":
      return "Hidden";
    default:
      return "Not Steam";
  }
}

function ConnectionRow({
  label,
  detail,
  avatar,
  onDisconnect,
}: {
  label: string;
  detail: string;
  avatar?: AppView["steam"]["profile"];
  onDisconnect: () => Promise<void>;
}) {
  return (
    <div className="connection-row">
      {avatar ? (
        <Avatar profile={avatar} />
      ) : (
        <span className="avatar humble-avatar">H</span>
      )}
      <span>
        <strong>{label}</strong>
        <small>{detail} · connected</small>
      </span>
      <button aria-label={`Disconnect ${detail}`} onClick={onDisconnect}>
        ×
      </button>
    </div>
  );
}

function SummaryCard({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string;
  detail: string;
  tone: "blue" | "green" | "violet";
}) {
  return (
    <article className={`summary-card ${tone}`}>
      <span className="summary-dot" />
      <p>{label}</p>
      <strong>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}

function ShieldIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 3 5.5 5.6v5.2c0 4.4 2.7 7.9 6.5 10.2 3.8-2.3 6.5-5.8 6.5-10.2V5.6L12 3Z" />
      <path d="m9.2 12 1.8 1.8 3.9-4.1" />
    </svg>
  );
}

function SteamIcon() {
  return (
    <svg viewBox="0 0 40 40" aria-hidden="true">
      <circle cx="25.5" cy="14.5" r="6.5" />
      <circle cx="13" cy="27" r="4.4" />
      <path d="m16.7 24.6 5.6-7.1M5.5 23l5.1 2.2M16 29.5l-3.3 1.6" />
    </svg>
  );
}

function GridIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="4" y="4" width="6" height="6" rx="1" />
      <rect x="14" y="4" width="6" height="6" rx="1" />
      <rect x="4" y="14" width="6" height="6" rx="1" />
      <rect x="14" y="14" width="6" height="6" rx="1" />
    </svg>
  );
}

function LibraryIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M5 4.5h4v15H5zM10.5 4.5h4v15h-4zM16.4 4l3.1-.8 3.2 14.7-3.1.8z" />
    </svg>
  );
}

function PeopleIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="9" cy="9" r="3" />
      <circle cx="17" cy="10" r="2.5" />
      <path d="M3.5 19c.5-3.2 2.4-5 5.5-5s5 1.8 5.5 5M14 15.2c2.9-.7 5.4.7 6 3.8" />
    </svg>
  );
}

function LinkIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="m9.5 14.5 5-5M8 17H6.5a3.5 3.5 0 0 1 0-7H10M14 7h3.5a3.5 3.5 0 0 1 0 7H14" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="10.8" cy="10.8" r="6.3" />
      <path d="m15.5 15.5 4.3 4.3" />
    </svg>
  );
}

export default App;
