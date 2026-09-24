use dirlisting::{DiagnosticKind, ErrorKind, Format, Options, Size, Timestamp, parse};
const BASE: &str = "https://example.org/files/";
const JSON: &str = include_str!("fixtures/nginx.json");
const HTML: &str = include_str!("fixtures/apache-table.html");

#[test]
fn every_limit_has_a_boundary() {
    let mut options = Options::default();
    options.limits.decoded_bytes = JSON.len();
    assert!(parse(JSON.as_bytes(), BASE, None, &options).is_ok());
    options.limits.decoded_bytes -= 1;
    assert_eq!(
        parse(JSON.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    options = Options::default();
    options.limits.depth = 2;
    assert!(parse(JSON.as_bytes(), BASE, None, &options).is_ok());
    options.limits.depth = 1;
    assert_eq!(
        parse(JSON.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    options = Options::default();
    options.limits.source_records = 1;
    let doubled = JSON.trim().strip_suffix(']').unwrap().to_owned() + ",null]";
    assert_eq!(
        parse(doubled.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    options = Options::default();
    options.limits.nodes = 0;
    for body in [JSON, HTML, "<list/>"] {
        assert_eq!(
            parse(body.as_bytes(), BASE, None, &options)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
    options = Options::default();
    options.limits.field_bytes = 32;
    let oversized = JSON.replace("a b.txt", &"a".repeat(33));
    assert_eq!(
        parse(oversized.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let exact = JSON.replace("a b.txt", &"a".repeat(32));
    assert!(parse(exact.as_bytes(), BASE, None, &options).is_ok());
}

#[test]
fn omitted_metadata_stays_absent_in_recognized_json() {
    let body = JSON.trim().strip_suffix(']').unwrap().to_owned()
        + ", {\"name\":\"no-metadata\",\"type\":\"file\"}]";
    let result = parse(body.as_bytes(), BASE, None, &Options::default()).unwrap();
    assert_eq!(result.entries.len(), 2);
    assert!(!result.partial);
    assert!(result.entries[1].size.is_none());
    assert!(result.entries[1].modified.is_none());
}

#[test]
fn root_relative_parent_and_literal_parent_name_are_distinct() {
    let simple = include_str!("fixtures/apache-list.html").replace("href=\"../\"", "href=\"/\"");
    assert_eq!(
        parse(simple.as_bytes(), BASE, None, &Options::default())
            .unwrap()
            .entries
            .len(),
        1
    );
    let body = HTML
        .replace("href=\"../\"", "href=\"/\"")
        .replace("a b.txt", "Parent Directory");
    let result = parse(body.as_bytes(), BASE, None, &Options::default()).unwrap();
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].name, "Parent Directory");
}

#[test]
fn malformed_records_are_not_empty_success() {
    let body =
        "<h1>Index of /</h1><hr><pre><a href='../'>../</a>\nlost record without a link\n</pre><hr>";
    assert_eq!(
        parse(body.as_bytes(), BASE, None, &Options::default())
            .unwrap_err()
            .kind,
        ErrorKind::MalformedListing
    );
    let body = HTML.replace("<a href=\"a%20b.txt\">a b.txt</a>", "missing destination");
    let result = parse(body.as_bytes(), BASE, None, &Options::default()).unwrap();
    assert!(result.partial);
    assert_eq!(result.rejected_records, 1);
    let body = HTML.replace(
        "a b.txt</a>",
        "a b.txt</a><a href='different'>other record</a>",
    );
    assert_eq!(
        parse(body.as_bytes(), BASE, None, &Options::default())
            .unwrap_err()
            .kind,
        ErrorKind::MalformedListing
    );
}

#[test]
fn schemes_bases_credentials_and_input_validation() {
    let body = HTML.replace("a%20b.txt", "ftp://user:secret@other.test/a?x=1#f");
    let mut options = Options::default();
    let rejected = parse(body.as_bytes(), BASE, None, &options).unwrap();
    assert!(rejected.partial);
    assert_eq!(
        rejected.diagnostics[0].kind,
        DiagnosticKind::UnsupportedScheme
    );
    options.allowed_schemes.push("ftp".into());
    let accepted = parse(body.as_bytes(), BASE, None, &options).unwrap();
    assert_eq!(accepted.entries[0].url.username(), "user");
    assert_eq!(accepted.entries[0].url.password(), Some("secret"));
    assert_eq!(accepted.entries[0].url.query(), Some("x=1"));
    let invalid_base = HTML.replace(
        "<head>",
        "<head><base href='data:bad'><base href='https://ignored.test/'>",
    );
    let listing = parse(invalid_base.as_bytes(), BASE, None, &Options::default()).unwrap();
    assert!(listing.partial);
    assert_eq!(listing.base_url.as_str(), BASE);
    for url in ["relative", "file:///tmp", "javascript:bad"] {
        assert_eq!(
            parse(JSON.as_bytes(), url, None, &options)
                .unwrap_err()
                .kind,
            ErrorKind::InvalidInput
        );
    }
    let mismatch = Options {
        format_hint: Some(Format::CaddyJson),
        ..Options::default()
    };
    assert_eq!(
        parse(JSON.as_bytes(), BASE, None, &mismatch)
            .unwrap_err()
            .kind,
        ErrorKind::Unrecognized
    );
}

#[test]
fn xml_entities_offsets_dates_and_exact_sizes() {
    for malformed in ["<list x='1' x='2'/>", "<list x='unterminated></list>"] {
        assert_eq!(
            parse(malformed.as_bytes(), BASE, None, &Options::default())
                .unwrap_err()
                .kind,
            ErrorKind::MalformedListing
        );
    }
    let xml = "<list><file size='12345 bytes' mtime='2025-01-02'>a&amp;b&#37;.txt</file><file mtime='2025-01-02T03:04:05.120+05:30'>offset</file></list>";
    let listing = parse(xml.as_bytes(), BASE, None, &Options::default()).unwrap();
    assert_eq!(listing.entries[0].name, "a&b%.txt");
    assert_eq!(listing.entries[0].size, Some(Size::Exact(12345)));
    assert!(matches!(
        listing.entries[0].modified,
        Some(Timestamp::Date(_))
    ));
    assert!(matches!(
        listing.entries[1].modified,
        Some(Timestamp::Offset {
            offset_seconds: 19_800,
            ..
        })
    ));
}

#[test]
fn bom_conflicts_and_invalid_utf16_fail() {
    let mut body = vec![0xff, 0xfe];
    body.extend(JSON.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(
        parse(
            &body,
            BASE,
            Some("application/json;charset=UTF-8"),
            &Options::default()
        )
        .unwrap_err()
        .kind,
        ErrorKind::Encoding
    );
    assert!(
        parse(
            &body,
            BASE,
            Some("application/json;charset=utf-16"),
            &Options::default()
        )
        .is_ok()
    );
    body.push(0);
    assert_eq!(
        parse(&body, BASE, None, &Options::default())
            .unwrap_err()
            .kind,
        ErrorKind::Encoding
    );
}

#[test]
fn unmatched_end_tags_do_not_hide_depth() {
    for open in ["<div></bogus>", "<div/>", "<b><i>"] {
        let body = format!("{}{HTML}", open.repeat(300));
        assert_eq!(
            parse(body.as_bytes(), BASE, None, &Options::default())
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
}

#[test]
fn truncated_fixtures_never_panic() {
    for body in [
        JSON,
        HTML,
        include_str!("fixtures/nginx.xml"),
        include_str!("fixtures/caddy.html"),
    ] {
        for cut in (0..body.len()).step_by(7) {
            let prefix = &body.as_bytes()[..cut];
            let a = parse(prefix, BASE, None, &Options::default());
            let b = parse(prefix, BASE, None, &Options::default());
            assert_eq!(a, b);
            if let Ok(listing) = a {
                assert_eq!(
                    listing.source_records,
                    listing.accepted_records + listing.rejected_records
                );
                assert_eq!(listing.entries.len(), listing.accepted_records);
            }
        }
    }
}
