use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    ApacheTable,
    ApachePre,
    ApacheList,
    NginxHtml,
    NginxJson,
    NginxXml,
    FancyIndex,
    CaddyHtml,
    CaddyJson,
    Lighttpd,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Unrecognized,
    Ambiguous,
    MalformedListing,
    InvalidInput,
    Encoding,
    LimitExceeded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}
impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    InvalidUrl,
    UnsupportedScheme,
    InvalidSize,
    InvalidTimestamp,
    MalformedRecord,
    EncodingReplacement,
    ContentTypeMismatch,
    InvalidBase,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub record: Option<usize>,
    pub raw: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Other,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Size {
    Exact(u64),
    Approximate(String),
    Invalid(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Date,
    Minute,
    Second,
    Fractional,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timestamp {
    Offset {
        raw: String,
        offset_seconds: i32,
        precision: Precision,
    },
    Unspecified {
        raw: String,
        precision: Precision,
    },
    Date(String),
    Invalid(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationKind {
    UrlReference,
    LiteralFilename,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub destination: String,
    pub destination_kind: DestinationKind,
    pub url: Url,
    pub kind: EntryKind,
    pub size: Option<Size>,
    pub modified: Option<Timestamp>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub format: Format,
    pub evidence: Vec<String>,
    pub base_url: Url,
    pub entries: Vec<Entry>,
    pub diagnostics: Vec<Diagnostic>,
    pub partial: bool,
    pub source_records: usize,
    pub accepted_records: usize,
    pub rejected_records: usize,
}
#[derive(Debug, Clone)]
pub struct Limits {
    pub body_bytes: usize,
    pub decoded_bytes: usize,
    pub source_records: usize,
    pub entries: usize,
    pub field_bytes: usize,
    pub nodes: usize,
    pub depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            body_bytes: 4 * 1024 * 1024,
            decoded_bytes: 8 * 1024 * 1024,
            source_records: 20_000,
            entries: 20_000,
            field_bytes: 16_384,
            nodes: 200_000,
            depth: 128,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Options {
    pub limits: Limits,
    pub format_hint: Option<Format>,
    pub allow_partial: bool,
    pub lossy_decoding: bool,
    pub allowed_schemes: Vec<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            format_hint: None,
            allow_partial: true,
            lossy_decoding: false,
            allowed_schemes: vec!["http".into(), "https".into()],
        }
    }
}
