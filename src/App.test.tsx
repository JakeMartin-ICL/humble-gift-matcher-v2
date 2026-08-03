import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { initialAppView, type AppView } from "./types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

const connectedView: AppView = {
  developmentCache: true,
  steam: {
    phase: "connected",
    message: "Steam is connected.",
    error: null,
    remembered: true,
    qrImage: null,
    ownershipLoaded: true,
    ownedAppIds: [123],
    profile: {
      steamId: "76561198000000000",
      displayName: "Jake",
      avatarUrl: "https://avatars.steamstatic.com/fixture_medium.jpg",
    },
  },
  humble: {
    phase: "connected",
    message: "Humble library connected.",
    error: null,
    remembered: true,
  },
  entitlements: {
    phase: "complete",
    message: "2 available Steam entitlements found.",
    error: null,
    completedOrders: 4,
    totalOrders: 4,
    refreshedAt: 1_700_000_000,
    summary: {
      total: 3,
      available: 2,
      needsMapping: 1,
      revealed: 1,
      excluded: 0,
    },
    items: [
      {
        id: "one",
        mappingKey: "giftable-hero",
        name: "Giftable Hero",
        parentName: "Choice Collection",
        steamAppId: 123,
        steamName: "Giftable Hero",
        mappingSource: "humble",
        mappingCandidates: [],
        keyTypeLabel: "Steam",
        status: "available",
        reasons: ["Visible, unrevealed Steam entitlement."],
        purchaseUrl: "https://www.humblebundle.com/downloads?key=safe",
        regionRestricted: false,
        packageAmbiguity: false,
      },
      {
        id: "two",
        mappingKey: "ambiguous-quest",
        name: "Ambiguous Quest Deluxe",
        parentName: "Indie Bundle",
        steamAppId: null,
        steamName: null,
        mappingSource: null,
        mappingCandidates: [
          { appId: 456, name: "Ambiguous Quest", similarity: 0.91 },
        ],
        keyTypeLabel: "Steam",
        status: "needs_mapping",
        reasons: ["Humble did not provide a valid Steam AppID."],
        purchaseUrl: "https://www.humblebundle.com/downloads?key=safe-two",
        regionRestricted: true,
        packageAmbiguity: true,
      },
      {
        id: "three",
        mappingKey: "revealed",
        name: "Already Revealed",
        parentName: "Old Bundle",
        steamAppId: 789,
        steamName: "Already Revealed",
        mappingSource: "humble",
        mappingCandidates: [],
        keyTypeLabel: "Steam",
        status: "revealed",
        reasons: ["The key value has already been revealed on Humble."],
        purchaseUrl: "https://www.humblebundle.com/downloads?key=safe-three",
        regionRestricted: false,
        packageAmbiguity: false,
      },
    ],
  },
  wishlists: {
    phase: "complete",
    message: "1 gift match across 2 accessible wishlists.",
    error: null,
    peopleTotal: 2,
    peopleAccessible: 2,
    peopleInaccessible: 0,
    wishlistApps: 14,
    people: [
      {
        steamId: "76561198000000000",
        displayName: "Jake",
        avatarUrl: null,
        isSelf: true,
        wishlistAccess: "accessible",
        wishlistCount: 10,
      },
      {
        steamId: "76561198000000001",
        displayName: "Alice",
        avatarUrl: null,
        isSelf: false,
        wishlistAccess: "accessible",
        wishlistCount: 4,
      },
    ],
    wishlistItems: {
      "76561198000000000": [
        { appId: 10, name: "Aperture Desk Job" },
        { appId: 20, name: "Half-Life" },
      ],
      "76561198000000001": [
        { appId: 123, name: "Giftable Hero" },
        { appId: 234, name: "Portal" },
        { appId: 345, name: "Portal 2" },
        { appId: 456, name: "SteamWorld Dig" },
      ],
    },
    matches: [
      {
        entitlementId: "one",
        appId: 123,
        steamName: "Giftable Hero",
        wishers: [
          {
            steamId: "76561198000000001",
            displayName: "Alice",
            avatarUrl: null,
            isSelf: false,
            wishlistAccess: "accessible",
            wishlistCount: 4,
          },
        ],
      },
    ],
  },
  steamReviews: {
    phase: "complete",
    message: "1 Steam rating available.",
    error: null,
    completed: 1,
    total: 1,
    items: {
      123: {
        appId: 123,
        positivePercentage: 94,
        totalPositive: 940,
        totalNegative: 60,
        totalReviews: 1000,
        scoreDescription: "Very Positive",
      },
    },
  },
};

describe("account onboarding", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return initialAppView;
      return undefined;
    });
  });

  it("explains the local-first matching workflow", async () => {
    render(<App />);

    expect(
      screen.getByRole("heading", {
        name: "Turn spare keys into thoughtful gifts.",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Connect your libraries")).toBeInTheDocument();
    expect(screen.getByText(/Keys are never revealed/)).toBeInTheDocument();
    expect(screen.getByText("0 of 2")).toBeInTheDocument();
  });

  it("starts Steam QR login through the native boundary", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      screen.getByRole("button", { name: "Connect", hidden: false }),
    );

    expect(mockedInvoke).toHaveBeenCalledWith("start_steam_login", undefined);
  });

  it("renders the Steam QR approval state", async () => {
    const qrView: AppView = {
      ...initialAppView,
      steam: {
        ...initialAppView.steam,
        phase: "qr_ready",
        qrImage: "data:image/svg+xml;base64,fixture",
        message: "Scan with Steam Mobile.",
      },
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return qrView;
      return undefined;
    });

    render(<App />);

    expect(
      await screen.findByRole("img", { name: "Steam sign-in QR code" }),
    ).toHaveAttribute("src", qrView.steam.qrImage);
    expect(screen.getByText("Approve in Steam Mobile")).toBeInTheDocument();
  });

  it("supports the development Humble session fallback", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") {
        return {
          ...initialAppView,
          developmentCache: true,
        };
      }
      return undefined;
    });
    render(<App />);

    await user.click(
      await screen.findByRole("button", {
        name: "Use a session cookie instead",
      }),
    );
    await user.type(
      screen.getByLabelText("Development `_simpleauth_sess`"),
      "development-session-value",
    );
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(mockedInvoke).toHaveBeenCalledWith("connect_humble_with_cookie", {
      session: "development-session-value",
    });
  });

  it("shows the product shell after both services connect", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      return undefined;
    });

    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "Wishlist matches" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Jake")).toBeInTheDocument();
    expect(screen.getByText("Insecure development cache")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Gift-ready entitlements" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Giftable Hero").length).toBeGreaterThan(0);
    expect(screen.getByText("Ambiguous Quest Deluxe")).toBeInTheDocument();
    expect(screen.queryByText("Already Revealed")).not.toBeInTheDocument();
    expect(
      screen.getAllByLabelText("Already in your Steam library"),
    ).toHaveLength(2);
    expect(screen.getAllByText("2").length).toBeGreaterThan(0);
  });

  it("opens mapped entitlement details and native store links", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      if (command === "load_steam_game_details") {
        return {
          appId: 123,
          name: "Giftable Hero",
          summary: "A compact heroic adventure.",
          description: "Save the day and bring your friends.",
          genres: ["Adventure", "RPG"],
          features: ["Single-player", "Steam Achievements"],
          developers: ["Fixture Studio"],
          publishers: ["Fixture Publishing"],
          releaseDate: "1 Jul, 2026",
          headerImage: "https://shared.akamai.steamstatic.com/header.jpg",
          screenshots: [
            "https://shared.akamai.steamstatic.com/one.jpg",
            "https://shared.akamai.steamstatic.com/two.jpg",
          ],
          review: connectedView.steamReviews.items[123],
        };
      }
      return undefined;
    });
    render(<App />);

    const detailButtons = await screen.findAllByRole("button", {
      name: "View details for Giftable Hero",
    });
    await user.click(detailButtons[0]);

    const drawer = await screen.findByRole("dialog", {
      name: "Giftable Hero",
    });
    expect(within(drawer).getByText("94% positive")).toBeInTheDocument();
    expect(within(drawer).getByText("Adventure")).toBeInTheDocument();
    expect(
      within(drawer).getByText("Save the day and bring your friends."),
    ).toBeInTheDocument();
    expect(
      mockedInvoke,
    ).toHaveBeenCalledWith("load_steam_game_details", { appId: 123 });

    await user.click(within(drawer).getByRole("button", { name: /Open in Steam/ }));
    await user.click(within(drawer).getByRole("button", { name: /View on Humble/ }));
    expect(mockedInvoke).toHaveBeenCalledWith("open_steam_game", {
      appId: 123,
    });
    expect(mockedInvoke).toHaveBeenCalledWith("open_humble_entitlement", {
      url: "https://www.humblebundle.com/downloads?key=safe",
    });
  });

  it("links entitlement sources without opening unmatched game details", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      return undefined;
    });
    render(<App />);

    const sourceLinks = await screen.findAllByRole("button", {
      name: "Choice Collection",
    });
    await user.click(sourceLinks[0]);
    expect(mockedInvoke).toHaveBeenCalledWith("open_humble_entitlement", {
      url: "https://www.humblebundle.com/downloads?key=safe",
    });
    expect(
      screen.queryByRole("button", { name: "Ambiguous Quest Deluxe" }),
    ).not.toBeInTheDocument();
  });

  it("does not start entitlement loading before native session validation", async () => {
    const readyView: AppView = {
      ...connectedView,
      entitlements: initialAppView.entitlements,
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return readyView;
      return undefined;
    });

    render(<App />);

    await screen.findByRole("heading", { name: "Wishlist matches" });
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "refresh_humble_entitlements",
    );
  });

  it("keeps wishlists usable while Steam title matching continues", async () => {
    const user = userEvent.setup();
    const refiningView: AppView = {
      ...connectedView,
      wishlists: {
        ...connectedView.wishlists,
        message: "Checking unresolved titles on Steam (25/100)…",
      },
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return refiningView;
      return undefined;
    });

    render(<App />);

    expect(
      await screen.findByText("Checking unresolved titles on Steam (25/100)…"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /People/ }));
    await user.click(screen.getByRole("button", { name: /Alice/ }));
    expect(screen.getByText("Giftable Hero")).toBeInTheDocument();
    expect(screen.getByText("SteamWorld Dig")).toBeInTheDocument();
  });

  it("navigates between matches, people, and mapping corrections", async () => {
    const user = userEvent.setup();
    const viewWithAutomaticMatch: AppView = {
      ...connectedView,
      entitlements: {
        ...connectedView.entitlements,
        items: [
          ...connectedView.entitlements.items,
          {
            ...connectedView.entitlements.items[0],
            id: "automatic",
            mappingKey: "exact-title-match",
            name: "Exact Title Match",
            steamAppId: 321,
            steamName: "Exact Title Match",
            mappingSource: "automatic",
            reasons: ["Automatic high-confidence Steam title match."],
          },
          {
            ...connectedView.entitlements.items[0],
            id: "manual",
            mappingKey: "corrected-title",
            name: "Corrected Title",
            steamAppId: 654,
            steamName: "Corrected Steam Title",
            mappingSource: "manual",
            reasons: ["Steam mapping corrected locally."],
          },
        ],
      },
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return viewWithAutomaticMatch;
      return undefined;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "Wishlist matches" });
    const navigationItems = within(
      screen.getByRole("navigation", { name: "Main navigation" }),
    ).getAllByRole("button");
    expect(navigationItems[0]).toHaveTextContent("Matches");
    expect(navigationItems[1]).toHaveTextContent("Entitlements");

    await user.click(screen.getByRole("button", { name: /Entitlements/ }));
    expect(
      screen.getByRole("heading", { name: "Browse entitlements" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", {
        name: "Hidden gems in your Humble library",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("94% positive")).toBeInTheDocument();
    expect(screen.getByText("Humble title: Corrected Title")).toBeInTheDocument();
    expect(
      screen.queryByText("Humble title: Giftable Hero"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByLabelText("Already in your Steam library"),
    ).toHaveAttribute("data-tooltip", "Already in your Steam library");

    await user.click(screen.getByRole("button", { name: /People/ }));
    expect(
      screen.getByRole("heading", { name: "People and wishlist access" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Alice")).toBeInTheDocument();
    expect(screen.getByText("4 wished-for games")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Alice/ }));
    expect(
      screen.getByRole("button", { name: "All people" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Giftable Hero")).toBeInTheDocument();
    expect(screen.getByText("SteamWorld Dig")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "All people" }));
    expect(screen.getByText("Alice")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Mappings/ }));
    expect(
      screen.getByRole("heading", { name: "Steam mappings" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Steam mapping corrections" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Ambiguous Quest Deluxe")).toBeInTheDocument();
    expect(screen.queryByText("Giftable Hero")).not.toBeInTheDocument();
    expect(screen.queryByText("Exact Title Match")).not.toBeInTheDocument();
    expect(screen.queryByText("Corrected Title")).not.toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: "Show matched" }));
    expect(screen.getAllByText("Exact Title Match")).toHaveLength(2);
    expect(screen.getByText("Corrected Title")).toBeInTheDocument();
    expect(
      screen.getByText("1 automatic · 1 corrected", { exact: false }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Matches/ }));
    expect(
      screen.getByRole("heading", { name: "Games friends want" }),
    ).toBeInTheDocument();
  });

  it("sorts available entitlements by Steam rating or review volume", async () => {
    const user = userEvent.setup();
    const available = connectedView.entitlements.items[0];
    const discoveryView: AppView = {
      ...connectedView,
      entitlements: {
        ...connectedView.entitlements,
        items: [
          {
            ...available,
            id: "rating-a",
            name: "Rating A",
            steamName: "Rating A",
            steamAppId: 101,
          },
          {
            ...available,
            id: "rating-b",
            name: "Rating B",
            steamName: "Rating B",
            steamAppId: 102,
          },
          {
            ...available,
            id: "popular",
            name: "Popular",
            steamName: "Popular",
            steamAppId: 103,
          },
        ],
      },
      steamReviews: {
        phase: "complete",
        message: "3 Steam ratings available.",
        error: null,
        completed: 3,
        total: 3,
        items: {
          101: {
            appId: 101,
            positivePercentage: 95,
            totalPositive: 950,
            totalNegative: 50,
            totalReviews: 1000,
            scoreDescription: "Very Positive",
          },
          102: {
            appId: 102,
            positivePercentage: 95,
            totalPositive: 1900,
            totalNegative: 100,
            totalReviews: 2000,
            scoreDescription: "Very Positive",
          },
          103: {
            appId: 103,
            positivePercentage: 80,
            totalPositive: 8000,
            totalNegative: 2000,
            totalReviews: 10000,
            scoreDescription: "Very Positive",
          },
        },
      },
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return discoveryView;
      return undefined;
    });
    render(<App />);

    await user.click(
      await screen.findByRole("button", { name: /Entitlements/ }),
    );
    let rows = screen
      .getAllByRole("row")
      .slice(1)
      .map((row) => within(row).getAllByRole("cell")[0]);
    expect(rows.map((cell) => cell.textContent)).toEqual([
      expect.stringContaining("Rating B"),
      expect.stringContaining("Rating A"),
      expect.stringContaining("Popular"),
    ]);

    await user.click(screen.getByRole("button", { name: "Most reviewed" }));
    rows = screen
      .getAllByRole("row")
      .slice(1)
      .map((row) => within(row).getAllByRole("cell")[0]);
    expect(rows.map((cell) => cell.textContent)).toEqual([
      expect.stringContaining("Popular"),
      expect.stringContaining("Rating B"),
      expect.stringContaining("Rating A"),
    ]);
  });

  it("loads Steam review summaries only when entitlements are opened", async () => {
    const user = userEvent.setup();
    const unratedView: AppView = {
      ...connectedView,
      steamReviews: initialAppView.steamReviews,
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return unratedView;
      return undefined;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "Wishlist matches" });
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "load_steam_reviews",
      undefined,
    );

    await user.click(
      screen.getByRole("button", { name: /Entitlements/ }),
    );
    await waitFor(() =>
      expect(mockedInvoke).toHaveBeenCalledWith(
        "load_steam_reviews",
        undefined,
      ),
    );
  });

  it("can inspect all classified entitlements", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      return undefined;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "Gift-ready entitlements" });
    await user.click(screen.getByRole("button", { name: "All" }));

    expect(screen.getAllByText("Already Revealed").length).toBeGreaterThan(0);
    expect(screen.getByText("Revealed")).toBeInTheDocument();
  });

  it("can correct an uncertain Steam mapping from a local suggestion", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      return undefined;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "Gift-ready entitlements" });
    await user.click(screen.getByRole("button", { name: "Needs mapping" }));
    await user.click(
      screen.getByRole("button", { name: /Ambiguous Quest.*App 456.*91%/ }),
    );

    await waitFor(() =>
      expect(mockedInvoke).toHaveBeenCalledWith("set_entitlement_mapping", {
        mappingKey: "ambiguous-quest",
        appId: 456,
        steamName: "Ambiguous Quest",
      }),
    );
  });

  it("can search Steam by title when local fuzzy suggestions are insufficient", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      if (command === "search_steam_apps") {
        return [
          {
            appId: 654,
            name: "Ambiguous Quest: Complete",
            similarity: 0.78,
          },
        ];
      }
      return undefined;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "Wishlist matches" });
    await user.click(screen.getByRole("button", { name: /Mappings/ }));
    await user.click(screen.getByRole("button", { name: "Needs mapping" }));
    await user.click(screen.getByRole("button", { name: "Search Steam" }));

    await waitFor(() =>
      expect(mockedInvoke).toHaveBeenCalledWith("search_steam_apps", {
        query: "Ambiguous Quest Deluxe",
      }),
    );
    await user.click(
      await screen.findByRole("button", {
        name: /Ambiguous Quest: Complete.*App 654.*78% title match/,
      }),
    );
    expect(mockedInvoke).toHaveBeenCalledWith("set_entitlement_mapping", {
      mappingKey: "ambiguous-quest",
      appId: 654,
      steamName: "Ambiguous Quest: Complete",
    });
  });

  it("shows the complete filtered library beyond the former 250-item limit", async () => {
    const items = Array.from({ length: 251 }, (_, index) => ({
      ...connectedView.entitlements.items[0],
      id: `gift-${index + 1}`,
      name: `Gift ${index + 1}`,
      steamAppId: index + 1,
    }));
    const largeView: AppView = {
      ...connectedView,
      entitlements: {
        ...connectedView.entitlements,
        summary: {
          ...connectedView.entitlements.summary,
          total: items.length,
          available: items.length,
          needsMapping: 0,
        },
        items,
      },
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return largeView;
      return undefined;
    });

    render(<App />);

    expect(await screen.findByText("Gift 251")).toBeInTheDocument();
  });

  it("disconnects each account independently", async () => {
    const user = userEvent.setup();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_app_view") return connectedView;
      return undefined;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "Wishlist matches" });
    await user.click(screen.getByRole("button", { name: "Disconnect Steam" }));
    await user.click(screen.getByRole("button", { name: "Disconnect Library" }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("disconnect_steam", undefined);
      expect(mockedInvoke).toHaveBeenCalledWith("disconnect_humble", undefined);
    });
  });
});
