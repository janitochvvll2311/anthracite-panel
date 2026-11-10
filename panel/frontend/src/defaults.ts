import type { Contribution } from "@anthracite/panel";

/// The view the panel opens on first boot: Home, if a visible Home slot exists.
///
/// The app id is the stable identity; the tag is the fallback for a renamed
/// first-party build that keeps the `home` slot.
export function defaultView(
  views: Contribution[],
): Contribution | undefined {
  return views.find(
    (contribution) =>
      !contribution.slot.hidden &&
      (contribution.app.id === "org.anthracite.home" ||
        contribution.slot.tag === "home"),
  );
}
