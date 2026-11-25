# Security Policy

## Supported versions

The current `0.7.x` line receives security fixes. Older lines may receive
targeted fixes when the issue is critical, but they are not guaranteed.

## Reporting a vulnerability

Please **do not open a public issue** for a security vulnerability.

Use GitHub's private vulnerability reporting:

<https://github.com/janitochvvll2311/anthracite-panel/security/advisories/new>

Include, as far as possible:

- the affected version or commit;
- the component (`engine`, `panel`, `sdk`, an app or the demo);
- the platform and toolchain;
- reproduction steps or a proof of concept;
- the expected and observed impact;
- any suggested fix or mitigation.

Do not include real credentials, personal data or third-party secrets in the
report. If you need to demonstrate a secret leak, use synthetic values.

## Response

- Acknowledgement: within 3 business days.
- Initial triage: within 7 business days.
- Coordinated disclosure: normally within 90 days, or earlier once a fix is
  available and users have had a reasonable time to update.

We will credit reporters in the advisory unless you ask to remain anonymous.

## Scope

This policy covers the engine, the official panel, the SDK, the bundled apps,
the release scripts and the demo. It does not cover vulnerabilities in
third-party dependencies, although we will help route those reports upstream.

## Safe harbor

We will not pursue legal action for good-faith security research that follows
this policy, avoids privacy violations, avoids disrupting other users and gives
us a reasonable opportunity to fix the issue before publication.
