# Changelog

## 0.1.0

Initial release.

### Added

- Passive, runtime-independent parsing of response bytes, final URL, optional
  Content-Type, and parser options into owned Rust data structures.
- Ten fixture-defined compatibility profiles: Apache table, preformatted and
  simple-list HTML; nginx HTML, JSON and XML; FancyIndex HTML; Caddy HTML table
  and JSON; and lighttpd HTML table listings.
- Distinct errors for unrecognized, ambiguous, malformed, invalid-input,
  encoding, and resource-limit failures. Recognized empty listings succeed;
  empty JSON arrays require an explicit format hint.
- Detection evidence, categorized diagnostics, explicit partial-result status,
  and source/accepted/rejected record counts.
- Separate displayed labels, destinations and resolved URLs, including HTML
  base handling and distinct URL-reference versus literal-filename semantics.
- Exact integer sizes, approximate sizes, and timestamps preserving timezone
  availability and precision. Source order and duplicate records are preserved.
- Configurable limits and explicit decoding, scheme, and partial-parsing policies.
- Native and `wasm32-unknown-unknown` support, with a minimum Rust version of 1.88.
- Native tests, Wasm builds, minimum-version checks, packaging verification,
  and strict Clippy checks in CI.

### Compatibility scope

- Fixtures are reduced, hand-authored generated-output profiles, not captures
  from every server version. Arbitrary customized templates and Caddy grid HTML
  are outside the supported profiles.
- Empty Apache simple lists without identifying parent navigation remain
  unrecognized. Format identification does not establish server attribution.
- Conservative HTML processing limits may reject unusually malformed but
  browser-recoverable pages. See the README for the full decoding and parsing
  contract.
- No networking, recursive traversal, JavaScript bindings, or downstream FFI
  adapter is included.
