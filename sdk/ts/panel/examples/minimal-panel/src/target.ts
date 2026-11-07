// The second panel's transport is a local panel/engine. The Basic token is for
// that service, so the default target must be loopback; a remote target needs an
// explicit opt-in and is the operator's responsibility.

export function isLoopback(hostname: string): boolean {
  return (
    hostname === "localhost" ||
    hostname === "127.0.0.1" ||
    hostname === "::1" ||
    hostname === "[::1]"
  );
}

export function loopbackTarget(raw: string, allowRemote = false): URL {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error(`ANTH_PANEL is not a URL: ${raw}`);
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error(`ANTH_PANEL must be http(s), got ${url.protocol}`);
  }
  if (url.username || url.password) {
    throw new Error("ANTH_PANEL must not embed credentials");
  }
  if (!allowRemote && !isLoopback(url.hostname)) {
    throw new Error(
      `ANTH_PANEL=${raw} is not loopback; set ANTH_PANEL_ALLOW_REMOTE=1 to send the panel token to a remote host`,
    );
  }
  return url;
}
