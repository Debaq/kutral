export type ModeIO = "touch" | "voice" | "both";
export type DepthProfile = "casual" | "interested" | "demanding" | "cinephile";
export type DubPref = "subs_always" | "dub_always" | "depends" | "indifferent";
export type Personality = "direct" | "warm" | "cinephile";

export type IntentId =
  | "mood_match"
  | "mood_shift"
  | "background"
  | "marks_you"
  | "light"
  | "surprise"
  | "decide";

export interface VeraOption {
  id: string;
  label: string;
  description: string | null;
}

export interface VeraSetup {
  mode_io: ModeIO;
  depth_profile: DepthProfile;
  languages_known: string[];
  dub_pref: DubPref;
  platforms: string[];
  excluded_genres: string[];
  excluded_themes: string[];
  personality: Personality;
  completed_at: number;
}

export const LANG_OPTIONS: VeraOption[] = [
  { id: "es", label: "Español", description: null },
  { id: "en", label: "Inglés", description: null },
  { id: "pt", label: "Portugués", description: null },
  { id: "fr", label: "Francés", description: null },
  { id: "it", label: "Italiano", description: null },
  { id: "other", label: "Otro", description: null },
];
