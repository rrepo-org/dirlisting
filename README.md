# dirlisting

A passive, pure-Rust library for recognizing HTTP directory listings. Supply the
already-decompressed response body, final response URL, optional Content-Type,
and options. No HTTP client, runtime, filesystem, browser, JavaScript API, or
Cloudflare API is required. Results are owned Rust data, usable by a downstream
application's own serialization/FFI layer.

Requires **Rust 1.88 or newer**. Add it to your application with:

```toml
[dependencies]
dirlisting = "0.1"
```

```rust
use dirlisting::{parse, Options, Format};

let response = br#"<list><file size="12345"
    mtime="2025-01-02T03:04:05Z">notes.txt</file></list>"#;
let listing = parse(response, "https://example.org/files/",
                    Some("application/xml"), &Options::default())?;
assert_eq!(listing.format, Format::NginxXml);
assert_eq!(listing.entries[0].url.as_str(),
           "https://example.org/files/notes.txt");
# Ok::<(), dirlisting::Error>(())
```

## Recognition guarantee

`Ok(Listing)` means the body matches a supported structural listing profile.
An empty recognized listing succeeds. Unrelated content never becomes an empty
success merely because no file links were found. Format names describe
compatibility, **not proof of server software**.

The `ErrorKind` enum distinguishes `Unrecognized`, `Ambiguous`,
`MalformedListing`, `InvalidInput`, `Encoding`, and `LimitExceeded`.
`DiagnosticKind` identifies record-local failures and advisory observations.
Inspect enums rather than parsing human-readable messages.

Supported compatibility profiles, defined by `tests/fixtures/`:

| Format | Structural evidence |
| --- | --- |
| Apache table | Index heading/title, name/date/size headers, indexlist ID, directory summary, or Apache sorting controls |
| Apache preformatted | Index heading/title and Name / Last modified / Size columns in a preformatted container |
| Apache simple list | Index heading/title and parent navigation in a list of linked items |
| nginx HTML | Index heading/title, preformatted parent navigation, horizontal separators |
| nginx JSON | Array with name, file/directory/other type, and mtime schema |
| nginx XML | A single list root containing typed file/directory/other elements |
| `FancyIndex` | Index heading/title, list table ID and expected headers |
| Caddy HTML | Breadcrumbs and listing-wrapped table with expected headers |
| Caddy JSON | Array with name, url, `is_dir`, size and `mod_time` schema |
| lighttpd HTML | Index heading/title, listing table class and expected headers |

Multiple recognized HTML containers or conflicting JSON schemas are ambiguous.
Detection is deterministic; hints cannot override incompatible recognized
structure. A generic empty JSON array is ambiguous; set `format_hint` to
`NginxJson` or `CaddyJson` when the caller independently knows the format.
An empty root-level Apache simple list without identifying parent navigation
is deliberately unrecognized. Arbitrary customized templates and Caddy's grid
layout are outside these profiles.

## Decoding and response metadata

Supported encodings are UTF-8, UTF-16LE, UTF-16BE, and Windows-1252.
Content-Type parameter names and encoding labels are case-insensitive; quoted
charset values are accepted. `utf8`, `utf-16`, `iso-8859-1`, and `us-ascii` are
aliases (the latter two use the HTML-compatible Windows-1252 mapping).

1. A recognized BOM selects the encoding and is stripped.
2. An explicit charset selects the encoding when there is no BOM. A conflicting
   BOM/charset is an encoding error; `utf-16` permits either UTF-16 BOM.
3. Without either, decode as UTF-8. No locale or heuristic legacy fallback.

Unsupported declared charsets and invalid text are errors. Explicit
`lossy_decoding` permits replacement characters and marks successful output
partial with `EncodingReplacement`. HTML meta charset declarations do not
override this transport-level decoding contract. XML declarations may omit the
encoding or declare UTF-8; other XML encoding declarations are rejected.

Content-Type's media type is advisory. Missing Content-Type is acceptable; a
recognized body can override a wrong media type with `ContentTypeMismatch`.
This advisory alone does not mark the listing partial. HTTP content codings
(gzip, Brotli, etc.) must already have been removed by the caller.

## Entries and URL semantics

* Only records in the recognized container are extracted. Breadcrumbs, footer
  links and table sorting headers are excluded. Parent navigation is identified
  by its destination semantics, not filename substrings.
* Source order and duplicate records are preserved. Icon/filename links in one
  table row produce one entry. Conflicting row destinations are structural errors.
* `name` is the displayed label; `destination` is the actual destination.
  `destination_kind` distinguishes URL references from literal filenames.
* HTML and Caddy JSON URLs are resolved as references using `url::Url`, without
  manual percent decoding. Encoded slashes, queries and fragments retain their
  URL meaning. HTML entities are decoded by the HTML parser.
* nginx JSON/XML names are literal path segments: a literal `%2F` becomes
  `%252F`. Dot segments, empty filenames, and names containing `/` are rejected.
  Structured filenames are appended to the final URL's path as a directory;
  its query and fragment are removed.
* The first HTML `base[href]` is resolved against the final response URL.
  HTTP(S) bases become `base_url`; invalid/unsupported bases produce a diagnostic
  and retain the response URL. Subsequent base elements are ignored.
* Final response URLs must be absolute HTTP(S). Destinations default to HTTP(S),
  configurable via `allowed_schemes`. Unsupported destinations are rejected
  with diagnostics, never silently removed. Control characters are rejected.
* `url::Url` exposes scheme, host, port, credentials, path, query and fragment
  for caller policy decisions. Successful parsing does not assert fetch safety.
* HTML trailing-slash destinations indicate directories. Other HTML entries
  are `Unknown`; structured formats provide explicit file/directory/other types.

## Metadata and partial results

Missing metadata is `None`. Invalid metadata is not replaced with guessed
values. `Size::Exact(u64)` is distinct from `Size::Approximate(String)` and
`Size::Invalid(String)`. Downstream JavaScript serializers should encode exact
integers as decimal strings or BigInt-compatible values rather than `Number`.

Timestamps retain original text, whether an offset was known, and precision.
RFC3339 and RFC2822 offsets are preserved; common Apache/nginx/lighttpd local
date-times have `Unspecified` timezone. ISO date-only values stay date-only.
Invalid values remain `Timestamp::Invalid` with diagnostics. No host timezone,
locale, or current time is consulted. Caddy HTML's machine-readable size and
time attributes take precedence over rendered approximations.

By default, trustworthy listings with record-local failures return partial
success. `source_records == accepted_records + rejected_records`;
`entries.len() == accepted_records`. Navigation records are excluded from these
counts but consume the record budget. Diagnostics use one-based source-record
indices; response-level diagnostics have no index. Optional metadata failures
mark output partial without rejecting the entry. `allow_partial = false`
turns non-advisory diagnostics into `MalformedListing` errors.

Broken JSON/XML syntax and unreliable HTML record boundaries fail outright.
The HTML5 tree builder tolerates common omitted closing tags. Preformatted
records must remain line-oriented. XML DTDs/entities requiring resolution are
rejected; nothing external is loaded or executed.

## Resource limits

Defaults: 4 MiB body, 8 MiB decoded text, 20,000 examined records, 20,000 retained
entries, 16 KiB entry fields, 200,000 tokens/nodes, nesting depth 128.
All limits are configurable through `Options::limits`. Exceeding a limit
always returns `LimitExceeded`, including in partial mode—never silent truncation.
Rejected records and navigation consume budget before filtering.

Body limits bound whole-document allocations. Conservative HTML lexical
token/nesting limits run before HTML5 tree construction; comments/scripts also
consume the lexical budget. JSON nesting/token limits run before deserialization;
XML is streamed. Additional fixed nesting ceilings are 128 for JSON and 256 for
HTML. The lexical HTML check can reject unusually malformed but recoverable
documents; it does not attempt arbitrary browser-template compatibility.

## Verification

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo build --locked --target wasm32-unknown-unknown
cargo clippy --locked --target wasm32-unknown-unknown -- -D warnings
```

Clippy `all` and `pedantic` are denied in the manifest. CI runs native tests and
Wasm compilation. The core has no JavaScript bindings or downstream adapter
crate; consumers own their FFI and packaging.

Release history is recorded in [CHANGELOG.md](CHANGELOG.md).
