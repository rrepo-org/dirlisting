use crate::{Context, DiagnosticKind, EntryKind, Error, ErrorKind, Format, limit};
use quick_xml::{Reader, events::Event};
use serde_json::Value;

pub(crate) fn json(text: &str, c: &mut Context<'_>) -> Result<(), Error> {
    json_preflight(text, c)?;
    let value: Value = serde_json::from_str(text)
        .map_err(|_| Error::new(ErrorKind::MalformedListing, "invalid JSON syntax"))?;
    json_records(&value, c)
}
fn json_preflight(text: &str, c: &Context<'_>) -> Result<(), Error> {
    // serde_json also enforces its own recursion ceiling (128).
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut nodes = 0;
    let mut string_bytes = 0;
    for b in text.bytes() {
        if quoted {
            string_bytes += 1;
            limit(
                string_bytes <= c.options.limits.field_bytes.saturating_mul(6),
                "JSON string",
            )?;
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else if b == b'"' {
            quoted = true;
            string_bytes = 0;
        } else if matches!(b, b'[' | b'{') {
            depth += 1;
            limit(depth <= c.options.limits.depth.min(128), "JSON depth")?;
        } else if matches!(b, b']' | b'}') {
            depth = depth.saturating_sub(1);
        }
        if !quoted && matches!(b, b'[' | b'{' | b',' | b':') {
            nodes += 1;
            limit(nodes <= c.options.limits.nodes, "JSON tokens")?;
        }
    }
    Ok(())
}
fn nginx_schema(v: &Value) -> bool {
    v.get("name").is_some_and(Value::is_string)
        && v.get("type")
            .and_then(Value::as_str)
            .is_some_and(|s| matches!(s, "file" | "directory" | "other"))
        && v.get("mtime").is_some_and(Value::is_string)
}
fn caddy_schema(v: &Value) -> bool {
    v.get("name").is_some_and(Value::is_string)
        && v.get("url").is_some_and(Value::is_string)
        && v.get("is_dir").is_some_and(Value::is_boolean)
        && v.get("size").is_some_and(Value::is_number)
        && v.get("mod_time").is_some_and(Value::is_string)
}
fn json_records(value: &Value, c: &mut Context<'_>) -> Result<(), Error> {
    let rows = value
        .as_array()
        .ok_or_else(|| Error::new(ErrorKind::Unrecognized, "JSON listing must be an array"))?;
    limit(
        rows.len() <= c.options.limits.source_records,
        "source records",
    )?;
    let ng = rows.iter().any(nginx_schema);
    let ca = rows.iter().any(caddy_schema);
    let format = match (ng, ca, c.options.format_hint) {
        (true, true, _) => return Err(Error::new(ErrorKind::Ambiguous, "mixed JSON schemas")),
        (true, false, _) => Format::NginxJson,
        (false, true, _) => Format::CaddyJson,
        (false, false, Some(f @ (Format::NginxJson | Format::CaddyJson))) if rows.is_empty() => f,
        (false, false, _) if rows.is_empty() => {
            return Err(Error::new(ErrorKind::Ambiguous, "empty JSON array"));
        }
        _ => {
            return Err(Error::new(
                ErrorKind::Unrecognized,
                "no supported JSON schema",
            ));
        }
    };
    c.listing.format = format;
    c.listing
        .evidence
        .push("array with format-specific record schema".into());
    for row in rows {
        let n = c.record()?;
        let valid = row.get("name").is_some_and(Value::is_string)
            && if format == Format::NginxJson {
                row.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|s| matches!(s, "file" | "directory" | "other"))
            } else {
                row.get("url").is_some_and(Value::is_string)
                    && row.get("is_dir").is_some_and(Value::is_boolean)
            };
        if !valid {
            c.reject(
                n,
                DiagnosticKind::MalformedRecord,
                "record does not match listing schema",
            )?;
            continue;
        }
        let name = row["name"].as_str().unwrap_or_default();
        let kind = if format == Format::CaddyJson {
            if row["is_dir"].as_bool() == Some(true) {
                EntryKind::Directory
            } else {
                EntryKind::File
            }
        } else {
            match row["type"].as_str() {
                Some("file") => EntryKind::File,
                Some("directory") => EntryKind::Directory,
                _ => EntryKind::Other,
            }
        };
        let size = row
            .get("size")
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned));
        let (dest, literal, date) = if format == Format::NginxJson {
            (name, true, row["mtime"].as_str())
        } else {
            (
                row["url"].as_str().unwrap_or_default(),
                false,
                row["mod_time"].as_str(),
            )
        };
        let time_key = if format == Format::NginxJson {
            "mtime"
        } else {
            "mod_time"
        };
        if row
            .get(time_key)
            .is_some_and(|v| !v.is_string() && !v.is_null())
        {
            c.diag(
                DiagnosticKind::InvalidTimestamp,
                Some(n),
                "timestamp is not a string",
            )?;
        }
        c.entry(n, name, dest, literal, kind, (size.as_deref(), date))?;
    }
    Ok(())
}

type XmlRecord = (usize, EntryKind, Option<String>, Option<String>, String);
fn xml_text(current: &mut Option<XmlRecord>, s: &str, max: usize) -> Result<(), Error> {
    if let Some(row) = current {
        limit(row.4.len().saturating_add(s.len()) <= max, "XML name")?;
        row.4.push_str(s);
    } else if !s.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::MalformedListing,
            "text outside XML record",
        ));
    }
    Ok(())
}
fn xml_declaration(d: &quick_xml::events::BytesDecl<'_>) -> Result<(), Error> {
    if let Some(enc) = d.encoding() {
        let enc = enc.map_err(|_| Error::new(ErrorKind::Encoding, "XML encoding"))?;
        if !enc.eq_ignore_ascii_case(b"utf-8") {
            return Err(Error::new(
                ErrorKind::Encoding,
                "only UTF-8 XML declarations supported",
            ));
        }
    }
    Ok(())
}
fn xml_record(
    e: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<&[u8]>,
    c: &mut Context<'_>,
) -> Result<XmlRecord, Error> {
    let kind = match e.name().as_ref() {
        b"file" => EntryKind::File,
        b"directory" => EntryKind::Directory,
        b"other" => EntryKind::Other,
        _ => {
            return Err(Error::new(
                ErrorKind::MalformedListing,
                "unknown XML record",
            ));
        }
    };
    let n = c.record()?;
    let mut size = None;
    let mut modified = None;
    for attr in e.attributes() {
        let attr =
            attr.map_err(|_| Error::new(ErrorKind::MalformedListing, "invalid XML attribute"))?;
        let val = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|_| Error::new(ErrorKind::MalformedListing, "invalid XML entity"))?
            .into_owned();
        limit(val.len() <= c.options.limits.field_bytes, "XML attribute")?;
        match attr.key.as_ref() {
            b"size" => size = Some(val),
            b"mtime" => modified = Some(val),
            _ => {}
        }
    }
    Ok((n, kind, size, modified, String::new()))
}
fn xml_root(e: &quick_xml::events::BytesStart<'_>) -> Result<(), Error> {
    if e.attributes().next().is_some() {
        return Err(Error::new(
            ErrorKind::MalformedListing,
            "XML list root attributes are unsupported",
        ));
    }
    Ok(())
}
pub(crate) fn xml(text: &str, c: &mut Context<'_>) -> Result<(), Error> {
    let mut reader = Reader::from_str(text);
    let mut depth = 0;
    let mut nodes = 0;
    let mut root = false;
    let mut closed = false;
    let mut current: Option<XmlRecord> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| Error::new(ErrorKind::MalformedListing, "invalid XML"))?;
        nodes += 1;
        limit(nodes <= c.options.limits.nodes, "XML nodes")?;
        match event {
            Event::Start(e) => {
                depth += 1;
                limit(depth <= c.options.limits.depth, "XML depth")?;
                if depth == 1 && e.name().as_ref() == b"list" && !root && !closed {
                    xml_root(&e)?;
                    root = true;
                } else if depth == 2 && root {
                    current = Some(xml_record(&e, &reader, c)?);
                } else {
                    return Err(Error::new(
                        if root {
                            ErrorKind::MalformedListing
                        } else {
                            ErrorKind::Unrecognized
                        },
                        "unsupported XML structure",
                    ));
                }
            }
            Event::Empty(e) if depth == 0 && e.name().as_ref() == b"list" && !root => {
                xml_root(&e)?;
                root = true;
                closed = true;
            }
            Event::Text(t) => {
                let s = t
                    .decode()
                    .map_err(|_| Error::new(ErrorKind::Encoding, "XML text"))?;
                xml_text(&mut current, &s, c.options.limits.field_bytes)?;
            }
            Event::GeneralRef(r) => {
                let s = r
                    .decode()
                    .map_err(|_| Error::new(ErrorKind::MalformedListing, "XML reference"))?;
                let escaped = format!("&{s};");
                let decoded = quick_xml::escape::unescape(&escaped)
                    .map_err(|_| Error::new(ErrorKind::MalformedListing, "unknown XML entity"))?;
                xml_text(&mut current, &decoded, c.options.limits.field_bytes)?;
            }
            Event::End(_) => {
                if depth == 2 {
                    let (n, kind, size, date, name) = current.take().ok_or_else(|| {
                        Error::new(ErrorKind::MalformedListing, "missing XML record")
                    })?;
                    c.entry(
                        n,
                        &name,
                        &name,
                        true,
                        kind,
                        (size.as_deref(), date.as_deref()),
                    )?;
                }
                if depth == 1 {
                    closed = true;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Decl(d) => xml_declaration(&d)?,
            Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
            _ => {
                return Err(Error::new(
                    ErrorKind::MalformedListing,
                    "unsupported XML construct",
                ));
            }
        }
    }
    if !root {
        return Err(Error::new(ErrorKind::Unrecognized, "no XML list root"));
    }
    if !closed || depth != 0 {
        return Err(Error::new(ErrorKind::MalformedListing, "unclosed XML list"));
    }
    c.listing.format = Format::NginxXml;
    c.listing
        .evidence
        .push("XML list root and typed records".into());
    Ok(())
}
