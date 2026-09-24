use crate::{Context, DiagnosticKind, EntryKind, Error, ErrorKind, Format, limit};
use scraper::{ElementRef, Html, Selector};

fn selector(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}
fn text(e: ElementRef<'_>) -> String {
    e.text().collect::<String>().trim().to_owned()
}
fn parent(a: ElementRef<'_>) -> bool {
    matches!(a.value().attr("href"), Some("../" | ".."))
}
fn navigation_parent(a: ElementRef<'_>, c: &Context<'_>) -> bool {
    if parent(a) {
        return true;
    }
    let label = text(a).to_ascii_lowercase();
    matches!(
        label.as_str(),
        "parent directory" | "parent directory/" | "up" | "../"
    ) && a.value().attr("href").is_some_and(|href| {
        c.listing.base_url.join(href).ok() == c.listing.base_url.join("../").ok()
    })
}
fn heading(doc: &Html) -> bool {
    doc.select(&selector("h1, title"))
        .any(|e| text(e).to_ascii_lowercase().starts_with("index of "))
}

fn preflight(body: &str, c: &Context<'_>) -> Result<(), Error> {
    // Conservative lexical ceilings are enforced before the tolerant tree builder.
    // Every '<' consumes budget, including occurrences in comments and scripts.
    let mut tags = 0usize;
    let mut stack: Vec<String> = Vec::new();
    for part in body.split('<').skip(1) {
        tags += 1;
        limit(tags <= c.options.limits.nodes, "HTML tokens")?;
        let tag = part
            .split(['>', ' ', '\t', '\n', '\r'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if tag.starts_with('/') {
            if let Some(i) = stack.iter().rposition(|s| s == &tag[1..]) {
                stack.truncate(i);
            }
        } else if !tag.starts_with('!')
            && !tag.starts_with('?')
            && !matches!(
                tag.as_str(),
                "meta"
                    | "link"
                    | "base"
                    | "img"
                    | "br"
                    | "hr"
                    | "input"
                    | "wbr"
                    | "source"
                    | "area"
                    | "embed"
                    | "param"
                    | "col"
                    | "track"
                    | "path"
                    | "circle"
                    | "rect"
                    | "line"
                    | "polyline"
                    | "polygon"
                    | "ellipse"
            )
        {
            // HTML implicitly closes these repeated optional-end-tag elements.
            if matches!(tag.as_str(), "li" | "tr" | "td" | "th" | "p") {
                let previous = stack.iter().rposition(|s| {
                    s == &tag
                        || (matches!(tag.as_str(), "td" | "th")
                            && matches!(s.as_str(), "td" | "th"))
                });
                if let Some(i) = previous {
                    stack.truncate(i);
                }
            }
            stack.push(tag);
            limit(
                stack.len() <= c.options.limits.depth.min(256),
                "HTML lexical depth",
            )?;
        }
    }
    Ok(())
}

pub(crate) fn parse(body: &str, c: &mut Context<'_>) -> Result<(), Error> {
    preflight(body, c)?;
    let doc = Html::parse_document(body);
    limit(
        doc.tree.nodes().count() <= c.options.limits.nodes,
        "HTML nodes",
    )?;
    let indexed = heading(&doc);
    let mut candidates = tables(&doc, indexed)?;
    detect_plain(&doc, indexed, &mut candidates, c);
    if candidates.len() > 1 {
        return Err(Error::new(
            ErrorKind::Ambiguous,
            "multiple listing containers",
        ));
    }
    let (format, container) = candidates
        .pop()
        .ok_or_else(|| Error::new(ErrorKind::Unrecognized, "no supported HTML structure"))?;
    c.listing.format = format;
    c.listing.evidence = vec![
        format!("{format:?} structural profile"),
        format!("container: {}", container.value().name()),
    ];
    if let Some(base) = doc.select(&selector("base[href]")).next() {
        let raw = base.value().attr("href").unwrap_or("");
        limit(raw.len() <= c.options.limits.field_bytes, "HTML base")?;
        match c.listing.base_url.join(raw) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => c.listing.base_url = u,
            _ => c.diag(DiagnosticKind::InvalidBase, None, raw)?,
        }
    }
    match format {
        Format::ApacheList => extract_list(container, c),
        Format::ApachePre | Format::NginxHtml => extract_pre(container, c),
        _ => extract_table(container, c),
    }
}

fn tables(doc: &Html, indexed: bool) -> Result<Vec<(Format, ElementRef<'_>)>, Error> {
    let mut candidates = vec![];
    for table in doc.select(&selector("table")) {
        let id = table.value().attr("id").unwrap_or("");
        let class = table.value().attr("class").unwrap_or("");
        let headers: Vec<String> = table
            .select(&selector("th"))
            .map(|e| text(e).to_ascii_lowercase())
            .collect();
        let known = headers.iter().any(|s| s.contains("name"))
            && headers.iter().any(|s| s.contains("size"))
            && headers
                .iter()
                .any(|s| s.contains("modif") || s.contains("date"));
        if table
            .ancestors()
            .skip(1)
            .filter_map(ElementRef::wrap)
            .any(|e| e.value().name() == "table")
        {
            return Err(Error::new(
                ErrorKind::MalformedListing,
                "nested listing tables",
            ));
        }
        let caddy_container = table
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|e| e.value().classes().any(|s| s == "listing"));
        let format = if id == "indexlist" && indexed && known {
            Some(Format::ApacheTable)
        } else if id == "list" && indexed && known {
            Some(Format::FancyIndex)
        } else if class.split_whitespace().any(|s| s == "listing") && indexed && known {
            Some(Format::Lighttpd)
        } else if (caddy_container
            || id == "listing"
            || class.split_whitespace().any(|s| s == "file-listing"))
            && known
            && doc
                .select(&selector(".breadcrumbs, .breadcrumb"))
                .next()
                .is_some()
        {
            Some(Format::CaddyHtml)
        } else if indexed
            && known
            && (table
                .value()
                .attr("summary")
                .is_some_and(|s| s.contains("Directory Listing"))
                || table
                    .select(&selector("th a[href]"))
                    .any(|a| a.value().attr("href").is_some_and(|s| s.starts_with("?C="))))
        {
            Some(Format::ApacheTable)
        } else {
            None
        };
        if let Some(f) = format {
            candidates.push((f, table));
        }
    }
    Ok(candidates)
}
fn detect_plain<'a>(
    doc: &'a Html,
    indexed: bool,
    candidates: &mut Vec<(Format, ElementRef<'a>)>,
    c: &Context<'_>,
) {
    if indexed {
        for pre in doc.select(&selector("pre")) {
            let t = text(pre);
            let has_parent = pre.select(&selector("a[href]")).any(parent);
            let apache = t.contains("Last modified") && t.contains("Size") && t.contains("Name");
            let nginx = has_parent && !apache && doc.select(&selector("hr")).count() >= 2;
            if apache {
                candidates.push((Format::ApachePre, pre));
            } else if nginx {
                candidates.push((Format::NginxHtml, pre));
            }
        }
        for ul in doc.select(&selector("ul")) {
            if ul
                .select(&selector("li > a[href]"))
                .any(|a| navigation_parent(a, c))
            {
                candidates.push((Format::ApacheList, ul));
            }
        }
    }
}
fn extract_list(container: ElementRef<'_>, c: &mut Context<'_>) -> Result<(), Error> {
    for li in container.select(&selector("li")) {
        let n = c.record()?;
        let Some(a) = li.select(&selector("a[href]")).next() else {
            c.reject(n, DiagnosticKind::MalformedRecord, "list item without link")?;
            continue;
        };
        if navigation_parent(a, c) {
            c.listing.source_records -= 1;
            continue;
        }
        add(c, n, a, None, None)?;
    }
    Ok(())
}
fn extract_pre(container: ElementRef<'_>, c: &mut Context<'_>) -> Result<(), Error> {
    validate_pre_lines(container)?;
    let format = c.listing.format;
    for a in container.select(&selector("a[href]")) {
        if a.ancestors()
            .filter_map(ElementRef::wrap)
            .any(|e| matches!(e.value().name(), "script" | "style"))
        {
            continue;
        }
        let href = a.value().attr("href").unwrap_or("");
        let n = c.record()?;
        if navigation_parent(a, c)
            || (format == Format::ApachePre && href.starts_with("?C="))
            || text(a).is_empty()
        {
            c.listing.source_records -= 1;
            continue;
        }
        let mut suffix = String::new();
        for sibling in a.next_siblings() {
            if let Some(t) = sibling.value().as_text() {
                suffix.push_str(t);
                if t.contains('\n') {
                    break;
                }
            } else {
                break;
            }
        }
        let line = suffix.lines().next().unwrap_or("").trim();
        let fields: Vec<&str> = line.split_whitespace().collect();
        let date = if fields.len() >= 2 {
            Some(format!("{} {}", fields[0], fields[1]))
        } else if !line.is_empty() {
            Some(line.to_owned())
        } else {
            None
        };
        let size = fields.get(2).copied();
        add(c, n, a, size, date.as_deref())?;
    }
    Ok(())
}
fn validate_pre_lines(container: ElementRef<'_>) -> Result<(), Error> {
    let mut anchored = false;
    let mut line = String::new();
    let validate = |line: &str, anchored: bool| {
        if line.trim().is_empty()
            || anchored
            || (line.contains("Name") && line.contains("Last modified") && line.contains("Size"))
        {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::MalformedListing,
                "unbounded text record in preformatted listing",
            ))
        }
    };
    for child in container.children() {
        if let Some(t) = child.value().as_text() {
            for (i, segment) in t.split('\n').enumerate() {
                if i != 0 {
                    validate(&line, anchored)?;
                    anchored = false;
                    line.clear();
                }
                line.push_str(segment);
            }
        } else if let Some(e) = ElementRef::wrap(child) {
            match e.value().name() {
                "a" => {
                    anchored = true;
                }
                "hr" | "br" => {
                    validate(&line, anchored)?;
                    anchored = false;
                    line.clear();
                }
                "img" => {}
                _ => {
                    return Err(Error::new(
                        ErrorKind::MalformedListing,
                        "unsupported preformatted record structure",
                    ));
                }
            }
        }
    }
    validate(&line, anchored)
}
fn extract_table(container: ElementRef<'_>, c: &mut Context<'_>) -> Result<(), Error> {
    let headers: Vec<String> = container
        .select(&selector("th"))
        .map(|e| text(e).to_ascii_lowercase())
        .collect();
    let size_col = headers.iter().position(|s| s.contains("size"));
    let date_col = headers
        .iter()
        .position(|s| s.contains("modif") || s.contains("date"));
    for row in container.select(&selector("tr")) {
        let n = c.record()?;
        if row.select(&selector("th")).next().is_some() {
            c.listing.source_records -= 1;
            continue;
        }
        let cells: Vec<_> = row.select(&selector("td")).collect();
        if cells.is_empty() || row.select(&selector("hr")).next().is_some() {
            c.listing.source_records -= 1;
            continue;
        }
        let anchors: Vec<_> = row
            .select(&selector("a[href]"))
            .filter(|a| !text(*a).is_empty())
            .collect();
        let Some(a) = anchors.last().copied() else {
            c.reject(
                n,
                DiagnosticKind::MalformedRecord,
                "row without filename link",
            )?;
            continue;
        };
        if navigation_parent(a, c) {
            c.listing.source_records -= 1;
            continue;
        }
        if anchors
            .iter()
            .any(|other| other.value().attr("href") != a.value().attr("href"))
        {
            return Err(Error::new(
                ErrorKind::MalformedListing,
                "conflicting links in listing row",
            ));
        }
        let size = size_col.and_then(|i| cells.get(i)).map(|e| {
            e.value()
                .attr("data-size")
                .map_or_else(|| text(*e), str::to_owned)
        });
        let date = date_col.and_then(|i| cells.get(i)).map(|e| {
            e.select(&selector("time[datetime]"))
                .next()
                .and_then(|t| t.value().attr("datetime"))
                .map_or_else(|| text(*e), str::to_owned)
        });
        add(c, n, a, size.as_deref(), date.as_deref())?;
    }
    Ok(())
}
fn add(
    c: &mut Context<'_>,
    n: usize,
    a: ElementRef<'_>,
    size: Option<&str>,
    date: Option<&str>,
) -> Result<(), Error> {
    let name = text(a);
    let href = a.value().attr("href").unwrap_or("");
    if href.is_empty() {
        return c.reject(n, DiagnosticKind::InvalidUrl, href);
    }
    let kind = if href.split(['?', '#']).next().unwrap_or("").ends_with('/') {
        EntryKind::Directory
    } else {
        EntryKind::Unknown
    };
    c.entry(n, &name, href, false, kind, (size, date))
}
