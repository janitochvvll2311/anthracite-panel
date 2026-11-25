# How are npm package names scoped, and are unscoped aliases allowed?

- Date: 2026-11-25T21:42:01.083Z
- Feature: app-sdk (`app-sdk`)
- Status: accepted

## Question

How are npm package names scoped, and are unscoped aliases allowed?

## Decision

The canonical npm packages are @anth-panel/panel, @anth-panel/client, @anth-panel/cli, @anth-panel/create-app, @anth-panel/wit and @anth-panel/bun-plugin. Do not publish unscoped anth-* names at launch. If demand appears later, an unscoped alias may be published as a thin re-export of the scoped package, documented as an alias; the scoped package stays canonical. crates.io keeps the anth-* convention with anth-panel as the owner because crates.io has no scopes and does not reserve prefixes.

## Rationale

An npm scope is the only real namespace: it prevents impersonation and centralizes ownership. Unscoped anth-* names are ordinary packages with no prefix reservation. A later alias is cheap and can be added without changing the canonical package, but publishing both now would duplicate the supply chain and confuse users.

## Alternatives considered

- Publish only unscoped anth-* names; rejected because the prefix cannot be owned and anyone could impersonate the family.
- Publish scoped and unscoped packages now; rejected because two canonical names double the release surface and the documentation.
- Use a different npm scope per package; rejected because it fragments ownership and discovery.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
