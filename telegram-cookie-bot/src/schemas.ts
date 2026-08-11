import { z } from "zod";

export const oauthServiceSchema = z.enum([
  "anilist",
  "myanimelist",
  "shikimori",
  "bangumi",
  "mangabaka",
]);

export type OAuthService = z.infer<typeof oauthServiceSchema>;

export const OAUTH_SERVICES = oauthServiceSchema.options;

export const isOAuthService = (s: string): s is OAuthService =>
  oauthServiceSchema.safeParse(s).success;

export const OAUTH_SERVICE_NAMES: Record<OAuthService, string> = {
  anilist: "AniList",
  myanimelist: "MyAnimeList",
  shikimori: "Shikimori",
  bangumi: "Bangumi",
  mangabaka: "MangaBaka",
};

export const OAUTH_SERVICE_COLORS: Record<OAuthService, string> = {
  anilist: "#02a4d6",
  myanimelist: "#2e51a2",
  shikimori: "#faa623",
  bangumi: "#fc2d5e",
  mangabaka: "#8b5cf6",
};

export const OAUTH_CODE_SERVICES: OAuthService[] = [
  "anilist",
  "myanimelist",
  "shikimori",
  "bangumi",
  "mangabaka",
];
