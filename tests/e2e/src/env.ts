export type Credentials = {
  operator: string;
  password: string;
};

/// The panel has no anonymous mode: every spec except the auth ones needs a
/// session. The password comes from the environment so no secret lands in the
/// tree; `ANTH_PANEL_PASSWORD` is the generated password in
/// `<state>/credentials.txt`, not the one first-boot replaced.
export function credentials(): Credentials {
  const password = process.env.ANTH_PANEL_PASSWORD;
  if (!password) {
    throw new Error(
      "ANTH_PANEL_PASSWORD is required: the e2e suite signs in to a running " +
        "panel (see tests/e2e/README.md)",
    );
  }
  return { operator: process.env.ANTH_PANEL_OPERATOR ?? "operator", password };
}
