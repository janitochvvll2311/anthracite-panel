/**
 * Where the shell can navigate, in one place.
 *
 * The sidebar, the drawer and the tab strip all read this list, so a new
 * destination is added here and appears in all three. v0.1.0 derived the same
 * thing from themes and panels; v0.2.0 has no themes yet, so a destination is a
 * flat entry and the theme grouping arrives with the second one.
 *
 * Sections that v0.1.0 had and v0.2.0 has not ported are deliberately absent
 * rather than listed and disabled: a disabled link is a promise the panel
 * cannot keep yet.
 */

export interface Destination {
  /** Stable id. It is the tab id, the route and the hash, so it never changes. */
  id: string;
  /** Full title, used by the sidebar and as the accessible name. */
  label: string;
  /** Title for the drawer, where a narrow column cuts a long label. */
  shortLabel: string;
  /** Decorative glyph beside the label. */
  icon: string;
}

export const DESTINATIONS: readonly Destination[] = [
  { id: "host", label: "Host", shortLabel: "Host", icon: "▤" },
];

/** The destination a hash names, falling back to the first one. */
export function destinationFrom(hash: string): Destination {
  const id = hash.replace(/^#\/?/, "").trim();
  return DESTINATIONS.find((destination) => destination.id === id) ?? DESTINATIONS[0]!;
}

/** The hash for a destination, which is also what a link writes. */
export function hashFor(destination: Destination): string {
  return `#/${destination.id}`;
}

/** The id for a hash, normalised to a destination that exists. */
export function idFromHash(hash: string): string {
  return destinationFrom(hash).id;
}
