// The risk ladder the Manager uses to compare a frozen grant with the risk the
// app reaches in the current composition. The engine owns the numbers; this is
// the presentation order (read is the baseline and has no flag).

export const RISKS = ["write", "admin", "host", "package"] as const;
export type Risk = (typeof RISKS)[number];

export type Authority = {
  /// The risk frozen in the grant, or `none` when there is no grant.
  current: Risk | "read" | "none";
  /// The risk the app reaches in the current composition.
  effective: Risk | "read" | "none";
  /// Levels the effective risk requires.
  required: Risk[];
  /// Required levels the current grant has not accepted.
  newLevels: Risk[];
};

function indexOf(risk: string): number {
  if (risk === "read") return 0;
  const index = RISKS.indexOf(risk as Risk);
  // An unknown level is treated as the top of the ladder: the UI must not
  // under-approve because the engine sent a name it does not know.
  return index < 0 ? RISKS.length : index + 1;
}

/// The approval flags a risk requires: write, then admin, then host, then
/// package; read needs no flag.
export function requiredLevels(effective: string): Risk[] {
  const limit = indexOf(effective);
  return RISKS.filter((_, index) => index + 1 <= limit);
}

/// Compare the grant against the current composition. An app that is not
/// running has no effective risk; the current grant is the only fact left.
/// `required` comes from the engine's approval preview when available.
export function authorityDiff(
  grant: { risk: string; accepted: string[] } | undefined,
  effective: string | undefined,
  required?: Risk[],
): Authority {
  const current = grant ? (grant.risk as Authority["current"]) : "none";
  const target = (effective ?? grant?.risk ?? "none") as Authority["effective"];
  const levels =
    required ?? (target === "none" ? [] : requiredLevels(target));
  const accepted = grant?.accepted ?? [];
  return {
    current,
    effective: target,
    required: levels,
    newLevels: levels.filter((level) => !accepted.includes(level)),
  };
}
