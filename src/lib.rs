#![doc = include_str!("../README.md")]
mod html;
mod metadata;
mod model;
mod structured;
pub use model::*;
pub use url::Url;

use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};

pub(crate) fn limit(ok: bool, what: &str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::LimitExceeded, what))
    }
}

/// Recognize and parse a decoded HTTP response body. No I/O is performed.
///
/// # Errors
/// Returns a categorized error for invalid input/encoding, exceeded limits,
/// unrecognized or ambiguous responses, and structurally malformed listings.
pub fn parse(
    body: &[u8],
    final_url: &str,
    content_type: Option<&str>,
    options: &Options,
) -> Result<Listing, Error> {
    limit(body.len() <= options.limits.body_bytes, "body bytes")?;
    limit(final_url.len() <= options.limits.field_bytes, "final URL")?;
    if let Some(ct) = content_type {
        limit(ct.len() <= options.limits.field_bytes, "Content-Type")?;
    }
    let base = Url::parse(final_url)
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid final URL"))?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "final URL must be HTTP(S)",
        ));
    }
    let charset = content_type.and_then(|ct| {
        ct.split(';').skip(1).find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("charset")
                .then(|| value.trim().trim_matches(['"', '\'']).to_ascii_lowercase())
        })
    });
    let declared = match charset.as_deref() {
        None => None,
        Some("utf-8" | "utf8") => Some(UTF_8),
        Some("utf-16" | "utf-16le") => Some(UTF_16LE),
        Some("utf-16be") => Some(UTF_16BE),
        Some("windows-1252" | "iso-8859-1" | "us-ascii") => Some(WINDOWS_1252),
        Some(_) => return Err(Error::new(ErrorKind::Encoding, "unsupported charset")),
    };
    let (bom, skip) = encoding_rs::Encoding::for_bom(body).map_or((None, 0), |(e, n)| (Some(e), n));
    if bom.is_some()
        && declared.is_some()
        && bom != declared
        && charset.as_deref() != Some("utf-16")
    {
        return Err(Error::new(
            ErrorKind::Encoding,
            "BOM conflicts with charset",
        ));
    }
    let encoding = bom.or(declared).unwrap_or(UTF_8);
    let (text, replaced) = encoding.decode_without_bom_handling(&body[skip..]);
    limit(text.len() <= options.limits.decoded_bytes, "decoded bytes")?;
    if replaced && !options.lossy_decoding {
        return Err(Error::new(ErrorKind::Encoding, "invalid encoded text"));
    }
    let mut context = Context {
        options,
        examined: 0,
        listing: Listing {
            format: Format::NginxHtml,
            evidence: vec![],
            base_url: base,
            entries: vec![],
            diagnostics: vec![],
            partial: false,
            source_records: 0,
            accepted_records: 0,
            rejected_records: 0,
        },
    };
    if replaced {
        context.diag(
            DiagnosticKind::EncodingReplacement,
            None,
            "replacement characters",
        )?;
    }
    let text = text.trim();
    if text.starts_with('[') || text.starts_with('{') {
        structured::json(text, &mut context)?;
    } else if text.starts_with("<?xml") || text.starts_with("<list") {
        structured::xml(text, &mut context)?;
    } else {
        html::parse(text, &mut context)?;
    }
    if let Some(hint) = options.format_hint
        && context.listing.format != hint
    {
        return Err(Error::new(
            ErrorKind::Unrecognized,
            "format hint does not match",
        ));
    }
    check_content_type(content_type, &mut context)?;
    Ok(context.listing)
}
fn check_content_type(content_type: Option<&str>, context: &mut Context<'_>) -> Result<(), Error> {
    if let Some(ct) = content_type {
        let mime = ct
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let matches = match context.listing.format {
            Format::NginxJson | Format::CaddyJson => mime == "application/json",
            Format::NginxXml => matches!(mime.as_str(), "application/xml" | "text/xml"),
            _ => matches!(mime.as_str(), "text/html" | "application/xhtml+xml"),
        };
        if !matches {
            context.diag(DiagnosticKind::ContentTypeMismatch, None, ct)?;
        }
    }
    Ok(())
}

pub(crate) struct Context<'a> {
    pub options: &'a Options,
    pub listing: Listing,
    examined: usize,
}
impl Context<'_> {
    pub fn diag(
        &mut self,
        kind: DiagnosticKind,
        record: Option<usize>,
        raw: &str,
    ) -> Result<(), Error> {
        if !self.options.allow_partial && kind != DiagnosticKind::ContentTypeMismatch {
            return Err(Error::new(ErrorKind::MalformedListing, format!("{kind:?}")));
        }
        limit(
            raw.len() <= self.options.limits.field_bytes,
            "diagnostic field",
        )?;
        self.listing.partial |= kind != DiagnosticKind::ContentTypeMismatch;
        self.listing.diagnostics.push(Diagnostic {
            kind,
            record,
            raw: raw.into(),
        });
        Ok(())
    }
    pub fn record(&mut self) -> Result<usize, Error> {
        self.examined += 1;
        self.listing.source_records += 1;
        limit(
            self.examined <= self.options.limits.source_records,
            "source records (including navigation)",
        )?;
        Ok(self.listing.source_records)
    }
    pub fn reject(&mut self, n: usize, kind: DiagnosticKind, raw: &str) -> Result<(), Error> {
        self.listing.rejected_records += 1;
        self.diag(kind, Some(n), raw)
    }
    pub fn entry(
        &mut self,
        n: usize,
        name: &str,
        destination: &str,
        literal: bool,
        kind: EntryKind,
        metadata: (Option<&str>, Option<&str>),
    ) -> Result<(), Error> {
        let (size, modified) = metadata;
        for s in [Some(name), Some(destination), size, modified]
            .into_iter()
            .flatten()
        {
            limit(s.len() <= self.options.limits.field_bytes, "entry field")?;
        }
        if destination.chars().any(char::is_control) {
            return self.reject(n, DiagnosticKind::InvalidUrl, destination);
        }
        let resolved = if literal {
            let mut u = self.listing.base_url.clone();
            u.set_query(None);
            u.set_fragment(None);
            if name.is_empty() || matches!(name, "." | "..") || name.contains('/') {
                return self.reject(n, DiagnosticKind::InvalidUrl, destination);
            }
            if let Ok(mut segments) = u.path_segments_mut() {
                segments.pop_if_empty().push(name);
                if kind == EntryKind::Directory {
                    segments.push("");
                }
            }
            Ok(u)
        } else {
            self.listing.base_url.join(destination)
        };
        let Ok(url) = resolved else {
            return self.reject(n, DiagnosticKind::InvalidUrl, destination);
        };
        if !self
            .options
            .allowed_schemes
            .iter()
            .any(|s| s == url.scheme())
        {
            return self.reject(n, DiagnosticKind::UnsupportedScheme, destination);
        }
        let size = size
            .filter(|s| !s.trim().is_empty() && !matches!(s.trim(), "-" | "—"))
            .map(metadata::size);
        let modified = modified
            .filter(|s| !s.trim().is_empty() && !matches!(s.trim(), "-" | "—"))
            .map(metadata::timestamp);
        if let Some(Size::Invalid(raw)) = &size {
            self.diag(DiagnosticKind::InvalidSize, Some(n), raw)?;
        }
        if let Some(Timestamp::Invalid(raw)) = &modified {
            self.diag(DiagnosticKind::InvalidTimestamp, Some(n), raw)?;
        }
        limit(
            self.listing.entries.len() < self.options.limits.entries,
            "retained entries",
        )?;
        self.listing.entries.push(Entry {
            name: name.into(),
            destination: destination.into(),
            destination_kind: if literal {
                DestinationKind::LiteralFilename
            } else {
                DestinationKind::UrlReference
            },
            url,
            kind,
            size,
            modified,
        });
        self.listing.accepted_records += 1;
        Ok(())
    }
}
