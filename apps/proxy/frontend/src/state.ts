import type { ProxyHost, ProxyUpstream } from "./types";

export type Upsert<T> = {
  items: T[];
  error: string | null;
};

/// Adds a new host, or replaces an existing one only when the form is an edit.
/// An add with a taken id is refused: replacing it would drop its routes.
export function upsertHost(
  hosts: ProxyHost[],
  host: ProxyHost,
  replace: boolean,
): Upsert<ProxyHost> {
  const index = hosts.findIndex((candidate) => candidate.id === host.id);
  if (index >= 0 && !replace) {
    return {
      items: hosts,
      error: `A host called ${host.id} already exists; edit it or choose another id.`,
    };
  }
  const items = hosts.slice();
  if (index >= 0) items[index] = host;
  else items.push(host);
  return { items, error: null };
}

/// The same rule for upstreams: an add never overwrites an edited row.
export function upsertUpstream(
  upstreams: ProxyUpstream[],
  upstream: ProxyUpstream,
  replace: boolean,
): Upsert<ProxyUpstream> {
  const index = upstreams.findIndex(
    (candidate) => candidate.id === upstream.id,
  );
  if (index >= 0 && !replace) {
    return {
      items: upstreams,
      error: `An upstream called ${upstream.id} already exists; edit it or choose another id.`,
    };
  }
  const items = upstreams.slice();
  if (index >= 0) items[index] = upstream;
  else items.push(upstream);
  return { items, error: null };
}
