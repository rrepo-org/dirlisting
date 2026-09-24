use dirlisting::{
    DiagnosticKind, EntryKind, ErrorKind, Format, Listing, Options, Precision, Size, Timestamp,
    parse,
};

const BASE: &str = "https://example.org/files/";
const FIXTURES: &[(Format, &str)] = &[
    (
        Format::ApacheTable,
        include_str!("fixtures/apache-table.html"),
    ),
    (Format::ApachePre, include_str!("fixtures/apache-pre.html")),
    (
        Format::ApacheList,
        include_str!("fixtures/apache-list.html"),
    ),
    (Format::NginxHtml, include_str!("fixtures/nginx.html")),
    (Format::NginxJson, include_str!("fixtures/nginx.json")),
    (Format::NginxXml, include_str!("fixtures/nginx.xml")),
    (Format::FancyIndex, include_str!("fixtures/fancyindex.html")),
    (Format::CaddyHtml, include_str!("fixtures/caddy.html")),
    (Format::CaddyJson, include_str!("fixtures/caddy.json")),
    (Format::Lighttpd, include_str!("fixtures/lighttpd.html")),
];
fn run(body: &str) -> Listing {
    parse(body.as_bytes(), BASE, None, &Options::default()).unwrap()
}
fn nginx(rows: &str) -> String {
    format!("<h1>Index of /</h1><hr><pre><a href=\"../\">../</a>\n{rows}</pre><hr>")
}
fn json(name: &str) -> String {
    serde_json::json!([{"name":name,"type":"file","mtime":"2025-01-02T03:04:05Z","size":12345}])
        .to_string()
}

#[test]
fn all_ten_profiles() {
    for &(format, body) in FIXTURES {
        let listing = run(body);
        assert_eq!(listing.format, format);
        assert!(!listing.partial, "{format:?}: {:?}", listing.diagnostics);
        assert_eq!(
            (
                listing.source_records,
                listing.accepted_records,
                listing.rejected_records
            ),
            (1, 1, 0)
        );
        assert_eq!(listing.entries[0].name, "a b.txt");
        assert_eq!(
            listing.entries[0].url.as_str(),
            "https://example.org/files/a%20b.txt"
        );
        assert_eq!(listing, run(body));
    }
}
#[test]
fn empty_listings_are_recognized_but_empty_json_needs_hint() {
    for &(format, body) in FIXTURES {
        if matches!(format, Format::NginxJson | Format::CaddyJson) {
            assert_eq!(
                parse(b"[]", BASE, None, &Options::default())
                    .unwrap_err()
                    .kind,
                ErrorKind::Ambiguous
            );
            let o = Options {
                format_hint: Some(format),
                ..Options::default()
            };
            assert!(parse(b"[]", BASE, None, &o).unwrap().entries.is_empty());
        } else {
            let empty = body
                .lines()
                .filter(|line| !line.contains("a%20b.txt") && !line.contains("<list><file"))
                .collect::<Vec<_>>()
                .join("\n");
            let empty = if format == Format::NginxXml {
                "<list/>"
            } else {
                &empty
            };
            let listing = run(empty);
            assert_eq!(listing.format, format);
            assert!(listing.entries.is_empty(), "{format:?}");
        }
    }
}
#[test]
fn weak_markers_and_error_pages_fail() {
    for body in [
        "Index of /",
        "<h1>Index of /</h1>",
        "<html><h1>Login</h1><a href='/login'>Sign in</a></html>",
        "<h1>Not Found</h1>",
        "[{\"name\":\"x\",\"type\":\"file\"}]",
        "{}",
    ] {
        assert_eq!(
            parse(
                body.as_bytes(),
                BASE,
                Some("text/html"),
                &Options::default()
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unrecognized
        );
    }
    let overlap = format!("{}{}", FIXTURES[0].1, FIXTURES[3].1);
    assert_eq!(
        parse(overlap.as_bytes(), BASE, None, &Options::default())
            .unwrap_err()
            .kind,
        ErrorKind::Ambiguous
    );
}
#[test]
fn urls_use_destinations_and_preserve_semantics() {
    let rows = "<a href=\"a%2Fb%20c.txt?x=1&amp;y=2#frag\">truncated...</a>\n<a href=\"/root\">root</a>\n<a href=\"https://other.test/x\">other</a>\n<a href=\"Parent%20Directory..%3F\">Parent Directory..?</a>\n<a href=\"雪 %25.txt\">雪 %.txt</a>\n";
    let listing = run(&nginx(rows));
    assert_eq!(listing.entries.len(), 5);
    assert_eq!(
        listing.entries[0].url.as_str(),
        "https://example.org/files/a%2Fb%20c.txt?x=1&y=2#frag"
    );
    assert_eq!(listing.entries[0].name, "truncated...");
    assert_eq!(listing.entries[1].url.path(), "/root");
    assert_eq!(listing.entries[2].url.host_str(), Some("other.test"));
    let based = run(&format!(
        "<base href='https://base.test/new/'>{}",
        nginx("<a href='x'>x</a>\n")
    ));
    assert_eq!(based.entries[0].url.as_str(), "https://base.test/new/x");
    let literal = run(&json("100% %2F ?#雪.txt"));
    assert_eq!(
        literal.entries[0].url.path(),
        "/files/100%25%20%252F%20%3F%23%E9%9B%AA.txt"
    );
}
#[test]
fn partial_counts_and_strict_mode() {
    let body = nginx(
        "<a href='ok'>ok</a>\n<a href='javascript:alert(1)'>bad</a>\n<a href='http://['>bad</a>\n",
    );
    let result = run(&body);
    assert!(result.partial);
    assert_eq!(
        (
            result.source_records,
            result.accepted_records,
            result.rejected_records
        ),
        (3, 1, 2)
    );
    assert_eq!(
        result.diagnostics[0].kind,
        DiagnosticKind::UnsupportedScheme
    );
    assert_eq!(result.diagnostics[1].kind, DiagnosticKind::InvalidUrl);
    let options = Options {
        allow_partial: false,
        ..Options::default()
    };
    assert_eq!(
        parse(body.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::MalformedListing
    );
    let mixed = json("ok").replace(']', ",null,{}]");
    let result = run(&mixed);
    assert_eq!(result.rejected_records, 2);
}
#[test]
fn malformed_syntax_is_not_reconstructed() {
    for body in [
        "[{",
        "[1,]",
        "<list><file>x</list>",
        "<list><file>x",
        "<list><file>&external;</file></list>",
        "<?xml version='1.0'?><!DOCTYPE list SYSTEM 'file:///secret'><list/>",
    ] {
        assert_eq!(
            parse(body.as_bytes(), BASE, None, &Options::default())
                .unwrap_err()
                .kind,
            ErrorKind::MalformedListing,
            "{body}"
        );
    }
    let recoverable = FIXTURES[0].1.replace("</td>", "");
    assert_eq!(run(&recoverable).entries.len(), 1);
}
#[test]
fn metadata_precision_absence_and_invalid_values() {
    assert!(matches!(
        run(FIXTURES[0].1).entries[0].size,
        Some(Size::Approximate(_))
    ));
    assert!(matches!(
        run(FIXTURES[0].1).entries[0].modified,
        Some(Timestamp::Unspecified {
            precision: Precision::Minute,
            ..
        })
    ));
    assert!(matches!(
        run(FIXTURES[4].1).entries[0].modified,
        Some(Timestamp::Offset {
            offset_seconds: 0,
            ..
        })
    ));
    let large = json("large").replace("12345", "18446744073709551615");
    assert_eq!(run(&large).entries[0].size, Some(Size::Exact(u64::MAX)));
    let invalid = large
        .replace("18446744073709551615", "18446744073709551616")
        .replace("2025-01-02T03:04:05Z", "bad");
    let result = run(&invalid);
    assert!(result.partial);
    assert!(matches!(result.entries[0].size, Some(Size::Invalid(_))));
    assert!(matches!(
        result.entries[0].modified,
        Some(Timestamp::Invalid(_))
    ));
    assert!(run(FIXTURES[2].1).entries[0].modified.is_none());
}
#[test]
fn encoding_and_content_type() {
    let body = json("café");
    let wrong = parse(
        body.as_bytes(),
        BASE,
        Some("text/html; charset=\"UTF-8\""),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(
        wrong.diagnostics[0].kind,
        DiagnosticKind::ContentTypeMismatch
    );
    assert!(!wrong.partial);
    assert_eq!(
        parse(
            body.as_bytes(),
            BASE,
            Some("application/json; charset=shift_jis"),
            &Options::default()
        )
        .unwrap_err()
        .kind,
        ErrorKind::Encoding
    );
    let (encoded, _, _) = encoding_rs::WINDOWS_1252.encode(&body);
    assert_eq!(
        parse(
            &encoded,
            BASE,
            Some("application/json; charset=windows-1252"),
            &Options::default()
        )
        .unwrap()
        .entries[0]
            .name,
        "café"
    );
    assert_eq!(
        parse(&encoded, BASE, None, &Options::default())
            .unwrap_err()
            .kind,
        ErrorKind::Encoding
    );
    let options = Options {
        lossy_decoding: true,
        ..Options::default()
    };
    assert!(parse(&encoded, BASE, None, &options).unwrap().partial);
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend(body.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(
        parse(&utf16, BASE, None, &Options::default())
            .unwrap()
            .entries[0]
            .name,
        "café"
    );
}
#[test]
fn duplicates_and_entry_types() {
    let duplicate = nginx("<a href='x'>x</a>\n<a href='x'>x</a>\n");
    assert_eq!(run(&duplicate).entries.len(), 2);
    for (ty, expected) in [
        ("file", EntryKind::File),
        ("directory", EntryKind::Directory),
        ("other", EntryKind::Other),
    ] {
        assert_eq!(
            run(&json("x").replace("\"file\"", &format!("\"{ty}\""))).entries[0].kind,
            expected
        );
    }
}
#[test]
fn limits_include_rejected_and_navigation_records() {
    let body = json("ok");
    let mut options = Options::default();
    options.limits.body_bytes = body.len();
    options.limits.source_records = 1;
    options.limits.entries = 1;
    assert!(parse(body.as_bytes(), BASE, None, &options).is_ok());
    options.limits.body_bytes -= 1;
    assert_eq!(
        parse(body.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    options.limits.body_bytes += 1;
    options.limits.entries = 0;
    assert_eq!(
        parse(body.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let many = json("ok").replace(']', &format!(",{}]", vec!["null"; 100].join(",")));
    options.limits.body_bytes = 10000;
    assert_eq!(
        parse(many.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let nav = nginx("<a href='../'>../</a>\n");
    assert_eq!(
        parse(nav.as_bytes(), BASE, None, &options)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let deep = format!("{}{}", "<div>".repeat(1000), "</div>".repeat(1000));
    assert_eq!(
        parse(deep.as_bytes(), BASE, None, &Options::default())
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
}
#[test]
fn adversarial_bytes_are_deterministic_and_do_not_panic() {
    let mut state = 1u32;
    for len in 0..256 {
        let body: Vec<u8> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                state.to_le_bytes()[0]
            })
            .collect();
        assert_eq!(
            parse(&body, BASE, None, &Options::default()),
            parse(&body, BASE, None, &Options::default())
        );
    }
}
