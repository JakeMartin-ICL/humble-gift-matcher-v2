export type ConnectionPhase =
  | "disconnected"
  | "connecting"
  | "qr_ready"
  | "waiting"
  | "connected"
  | "error";

export type SteamProfile = {
  steamId: string;
  displayName: string;
  avatarUrl: string | null;
};

export type ServiceConnection = {
  phase: ConnectionPhase;
  message: string;
  error: string | null;
  remembered: boolean;
};

export type SteamConnection = ServiceConnection & {
  qrImage: string | null;
  profile: SteamProfile | null;
  ownershipLoaded: boolean;
  ownedAppIds: number[];
};

export type HumbleConnection = ServiceConnection;

export type EntitlementStatus =
  | "available"
  | "needs_mapping"
  | "revealed"
  | "expired"
  | "hidden"
  | "not_steam";

export type Entitlement = {
  id: string;
  mappingKey: string;
  name: string;
  parentName: string;
  steamAppId: number | null;
  steamName: string | null;
  mappingSource: "humble" | "automatic" | "manual" | null;
  mappingCandidates: MappingCandidate[];
  keyTypeLabel: string;
  status: EntitlementStatus;
  reasons: string[];
  purchaseUrl: string | null;
  expirationDate: string | null;
  regionRestricted: boolean;
  packageAmbiguity: boolean;
};

export type MappingCandidate = {
  appId: number;
  name: string;
  similarity: number;
};

export type WishlistPerson = {
  steamId: string;
  displayName: string;
  avatarUrl: string | null;
  isSelf: boolean;
  wishlistAccess: "loading" | "accessible" | "inaccessible" | "error";
  wishlistCount: number;
};

export type WishlistGame = {
  appId: number;
  name: string;
};

export type GiftMatch = {
  entitlementId: string;
  appId: number;
  steamName: string;
  wishers: WishlistPerson[];
};

export type WishlistSync = {
  phase: "idle" | "loading" | "complete" | "error";
  message: string;
  error: string | null;
  peopleTotal: number;
  peopleAccessible: number;
  peopleInaccessible: number;
  wishlistApps: number;
  matches: GiftMatch[];
  people: WishlistPerson[];
  wishlistItems: Record<string, WishlistGame[]>;
};

export type EntitlementSummary = {
  total: number;
  available: number;
  needsMapping: number;
  revealed: number;
  excluded: number;
};

export type EntitlementSync = {
  phase: "idle" | "loading" | "complete" | "error";
  message: string;
  error: string | null;
  completedOrders: number;
  totalOrders: number;
  refreshedAt: number | null;
  summary: EntitlementSummary;
  items: Entitlement[];
};

export type SteamReviewSummary = {
  appId: number;
  positivePercentage: number | null;
  totalPositive: number;
  totalNegative: number;
  totalReviews: number;
  scoreDescription: string;
};

export type SteamGameDetails = {
  appId: number;
  name: string;
  summary: string;
  description: string;
  genres: string[];
  features: string[];
  developers: string[];
  publishers: string[];
  releaseDate: string | null;
  headerImage: string | null;
  screenshots: string[];
  review: SteamReviewSummary | null;
};

export type SteamReviewSync = {
  phase: "idle" | "loading" | "complete" | "error";
  message: string;
  error: string | null;
  completed: number;
  total: number;
  items: Record<number, SteamReviewSummary>;
};

export type AppView = {
  steam: SteamConnection;
  humble: HumbleConnection;
  entitlements: EntitlementSync;
  wishlists: WishlistSync;
  steamReviews: SteamReviewSync;
  developmentCache: boolean;
};

export const initialAppView: AppView = {
  steam: {
    phase: "disconnected",
    message: "Connect Steam to read your identity and accessible wishlists.",
    error: null,
    remembered: false,
    qrImage: null,
    profile: null,
    ownershipLoaded: false,
    ownedAppIds: [],
  },
  humble: {
    phase: "disconnected",
    message: "Sign in on Humble's own page to load available entitlements.",
    error: null,
    remembered: false,
  },
  entitlements: {
    phase: "idle",
    message: "Ready to load available Humble entitlements.",
    error: null,
    completedOrders: 0,
    totalOrders: 0,
    refreshedAt: null,
    summary: {
      total: 0,
      available: 0,
      needsMapping: 0,
      revealed: 0,
      excluded: 0,
    },
    items: [],
  },
  wishlists: {
    phase: "idle",
    message: "Ready to load accessible Steam wishlists.",
    error: null,
    peopleTotal: 0,
    peopleAccessible: 0,
    peopleInaccessible: 0,
    wishlistApps: 0,
    matches: [],
    people: [],
    wishlistItems: {},
  },
  steamReviews: {
    phase: "idle",
    message: "Steam ratings load when you browse entitlements.",
    error: null,
    completed: 0,
    total: 0,
    items: {},
  },
  developmentCache: false,
};
