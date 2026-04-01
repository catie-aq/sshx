/** Font definitions for text blocks and sticky notes. */

// Import font CSS
import "@fontsource-variable/inter";
import "@fontsource/merriweather/400.css";
import "@fontsource/merriweather/700.css";
import "@fontsource/caveat/400.css";
import "@fontsource/caveat/700.css";
import "@fontsource/permanent-marker/400.css";
import "@fontsource-variable/playfair-display";
import "@fontsource/space-mono/400.css";
import "@fontsource/space-mono/700.css";
import "@fontsource-variable/nunito";

export type FontDef = {
  id: string;
  label: string;
  family: string;
  category: string;
};

export const FONTS: FontDef[] = [
  { id: "inter", label: "Inter", family: "'Inter Variable', sans-serif", category: "Sans" },
  { id: "firacode", label: "Fira Code", family: "'Fira Code VF', 'Fira Code', monospace", category: "Mono" },
  { id: "merriweather", label: "Merriweather", family: "'Merriweather', serif", category: "Serif" },
  { id: "caveat", label: "Caveat", family: "'Caveat', cursive", category: "Hand" },
  { id: "permanent-marker", label: "Marker", family: "'Permanent Marker', cursive", category: "Impact" },
  { id: "playfair", label: "Playfair", family: "'Playfair Display Variable', serif", category: "Display" },
  { id: "space-mono", label: "Space Mono", family: "'Space Mono', monospace", category: "Mono" },
  { id: "nunito", label: "Nunito", family: "'Nunito Variable', sans-serif", category: "Sans" },
];

export const FONT_MAP: Record<string, FontDef> = Object.fromEntries(
  FONTS.map((f) => [f.id, f])
);

export const DEFAULT_FONT = "inter";

export function getFontFamily(fontId: string): string {
  return FONT_MAP[fontId]?.family ?? FONT_MAP[DEFAULT_FONT].family;
}
