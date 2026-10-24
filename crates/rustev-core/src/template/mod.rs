//! Pure `rustev.template/1` language machinery (spec 037, 3.3 and 3.4).
//! This module does not construct contract documents or authorize generation.
//! Callers supply resolved static types and exact values; graph provenance and
//! parameter binding checks belong to the future generation compiler.
mod check;
mod parse;
mod render;

use rustev_contract::{decimal::Decimal, time::Timestamp};
use std::collections::{BTreeMap, BTreeSet};

pub const LANGUAGE: &str = "rustev.template/1";
pub const HELPERS: &str = "rustev.template-helpers/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Syntax,
    SourceBound,
    Nesting,
    InvalidBind,
    UnknownPath(String),
    ModelOutputFlow(String),
    NotExact(String),
    Type(String),
    InvalidDeclaration,
    UnsafeRaw,
    InvalidCurrency,
    OutputBound,
    MayBeEmpty,
    Arithmetic,
    MissingValue(String),
    InvalidValue(String),
    Canonical,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "template: {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Exact,
    Model,
    Mixed,
    Generated,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub kind: Type,
    pub class: Class,
}
impl Field {
    pub fn exact(kind: Type) -> Self {
        Self {
            kind,
            class: Class::Exact,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bool,
    Integer,
    Decimal,
    Text {
        max_bytes: usize,
    },
    Enum(Vec<String>),
    Timestamp,
    Maybe(Box<Field>),
    List {
        element: Box<Field>,
        max_items: usize,
    },
    Record(BTreeMap<String, Field>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
    Text(String),
    Enum(String),
    Timestamp(Timestamp),
    Maybe(Option<Box<Value>>),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
}
pub type Schema = BTreeMap<String, Field>;
pub type Values = BTreeMap<String, Value>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    Text,
    Html,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Refuse,
    Truncate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    Single,
    Multi,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Und,
    EnUs,
    EnGb,
    DeDe,
    FrFr,
    EsEs,
    PlPl,
}
impl Locale {
    pub fn parse(s: &str) -> Result<Self, Error> {
        match s {
            "und" => Ok(Self::Und),
            "en-US" => Ok(Self::EnUs),
            "en-GB" => Ok(Self::EnGb),
            "de-DE" => Ok(Self::DeDe),
            "fr-FR" => Ok(Self::FrFr),
            "es-ES" => Ok(Self::EsEs),
            "pl-PL" => Ok(Self::PlPl),
            _ => Err(Error::InvalidDeclaration),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Backstop { bind: Option<String> },
    Prompt,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub mode: Mode,
    pub locale: Locale,
    pub escape: Escape,
    pub overflow: Overflow,
    pub lines: Lines,
    pub max_bytes: usize,
    pub required: bool,
}
impl Options {
    /// Prompt instructions use the locale-neutral helper table and retain LF.
    pub fn prompt() -> Self {
        Self {
            mode: Mode::Prompt,
            locale: Locale::Und,
            escape: Escape::Text,
            overflow: Overflow::Refuse,
            lines: Lines::Multi,
            max_bytes: 16384,
            required: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min: usize,
    pub max: usize,
}
#[derive(Debug, Clone)]
enum Node {
    Text(String),
    Emit(String, Format),
    Section(String, bool, Vec<Node>),
}
#[derive(Debug, Clone)]
enum Format {
    Plain,
    Raw,
    Number(u32),
    Money(u32, Currency),
    Date(bool),
    Join,
}
#[derive(Debug, Clone)]
enum Currency {
    Literal(String),
    Path(String),
}
#[derive(Debug, Clone)]
pub struct Template {
    source: String,
    nodes: Vec<Node>,
    schema: Schema,
    options: Options,
    bounds: Bounds,
    references: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub truncated: bool,
    pub reads: BTreeSet<String>,
}
impl Template {
    pub fn compile(source: &str, schema: Schema, options: Options) -> Result<Self, Error> {
        check::options(&options, &schema)?;
        let limit = if options.mode == Mode::Prompt {
            16384
        } else {
            4096
        };
        if source.len() > limit {
            return Err(Error::SourceBound);
        }
        let nodes = parse::parse(source)?;
        let mut references = BTreeSet::new();
        let bounds = check::nodes(&nodes, &schema, &options, &mut references)?;
        if options.overflow == Overflow::Refuse && bounds.max > options.max_bytes {
            return Err(Error::OutputBound);
        }
        if options.required && bounds.min == 0 {
            return Err(Error::MayBeEmpty);
        }
        Ok(Self {
            source: source.into(),
            nodes,
            schema,
            options,
            bounds,
            references,
        })
    }
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
    pub fn references(&self) -> &BTreeSet<String> {
        &self.references
    }
    /// Preserve source bytes, including comments and tag spaces: every authored
    /// change must remain visible to the enclosing prompt document's identity.
    pub fn canonical_source(&self) -> &str {
        &self.source
    }
    pub fn render(&self, values: &Values) -> Result<Rendered, Error> {
        render::render(self, values)
    }
}
/// Canonicalize the complete supplied prompt document using the existing
/// contract writer. This does not validate or introduce a contract document type.
pub fn prompt_canonical<T: serde::Serialize>(document: &T) -> Result<Vec<u8>, Error> {
    rustev_contract::canonical::canonical_bytes(document).map_err(|_| Error::Canonical)
}
/// Digest a complete prompt document, including id, version and declarations.
/// The caller is responsible for document validation in the compilation layer.
pub fn prompt_digest<T: serde::Serialize>(document: &T) -> Result<String, Error> {
    Ok(rustev_contract::canonical::tagged_digest(
        "rustev.prompt-template/1",
        &prompt_canonical(document)?,
    ))
}
/// Check a literal list backstop against the resolved field vocabulary.
pub fn list_backstop(
    options: &[String],
    vocabulary: &[String],
    max_items: usize,
    required: bool,
) -> Result<Vec<String>, Error> {
    if !(1..=64).contains(&max_items)
        || vocabulary.is_empty()
        || vocabulary.len() > 256
        || vocabulary.iter().any(|s| s.is_empty() || s.len() > 64)
        || vocabulary.iter().collect::<BTreeSet<_>>().len() != vocabulary.len()
    {
        return Err(Error::InvalidDeclaration);
    }
    if required && options.is_empty() {
        return Err(Error::MayBeEmpty);
    }
    if options.len() > max_items {
        return Err(Error::OutputBound);
    }
    if options.iter().collect::<BTreeSet<_>>().len() != options.len()
        || options.iter().any(|s| !vocabulary.contains(s))
    {
        return Err(Error::InvalidDeclaration);
    }
    Ok(options.to_vec())
}
/// Pure syntax checks for the future prompt document's id and version.
/// Versions are three canonical unsigned decimal components, without suffixes.
pub fn prompt_header(id: &str, version: &str) -> Result<(), Error> {
    let valid_id = (1..=128).contains(&id.len())
        && id
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
    let parts: Vec<_> = version.split('.').collect();
    if !valid_id
        || parts.len() != 3
        || parts.iter().any(|s| {
            s.is_empty()
                || !s.bytes().all(|b| b.is_ascii_digit())
                || (s.len() > 1 && s.starts_with('0'))
        })
    {
        return Err(Error::InvalidDeclaration);
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingSource {
    Literal,
    Profile,
    ExactReference,
}
/// Check a resolved parameter binding's type and class. Reference location and
/// graph lineage are checked by the future compiler before calling this helper.
pub fn parameter_binding(
    parameter: &Type,
    supplied: &Field,
    source: BindingSource,
) -> Result<(), Error> {
    if supplied.class == Class::Generated {
        return Err(Error::ModelOutputFlow("parameter".into()));
    }
    if supplied.class != Class::Exact {
        return Err(Error::NotExact("parameter".into()));
    }
    let matches = match (parameter, &supplied.kind) {
        (Type::Bool, Type::Bool)
        | (Type::Integer, Type::Integer)
        | (Type::Decimal, Type::Decimal) => true,
        (Type::Enum(options), Type::Enum(actual)) => {
            !actual.is_empty() && actual.iter().all(|s| options.contains(s))
        }
        (Type::Text { max_bytes }, Type::Text { max_bytes: actual }) => {
            source != BindingSource::ExactReference && *max_bytes <= 1024 && actual <= max_bytes
        }
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(Error::Type("parameter".into()))
    }
}
