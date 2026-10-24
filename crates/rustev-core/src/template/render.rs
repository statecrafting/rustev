use super::*;
use rustev_contract::decimal::Rounding;

pub(super) fn escaped(s: &str, o: &Options) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let c = if c.is_control() && !(c == '\n' && o.lines == Lines::Multi) {
            ' '
        } else {
            c
        };
        if o.escape == Escape::Html {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn valid(v: &Value, t: &Type) -> bool {
    match (v, t) {
        (Value::Bool(_), Type::Bool)
        | (Value::Integer(_), Type::Integer)
        | (Value::Decimal(_), Type::Decimal)
        | (Value::Timestamp(_), Type::Timestamp) => true,
        (Value::Text(s), Type::Text { max_bytes }) => s.len() <= *max_bytes,
        (Value::Enum(s), Type::Enum(opts)) => opts.contains(s),
        (Value::Maybe(v), Type::Maybe(f)) => v.as_ref().is_none_or(|v| valid(v, &f.kind)),
        (Value::List(v), Type::List { element, max_items }) => {
            v.len() <= *max_items && v.iter().all(|v| valid(v, &element.kind))
        }
        (Value::Record(v), Type::Record(fields)) => fields
            .iter()
            .all(|(k, f)| v.get(k).is_some_and(|v| valid(v, &f.kind))),
        _ => false,
    }
}
fn value<'a>(
    p: &str,
    values: &BTreeMap<String, &'a Value>,
    schema: &Schema,
    o: &Options,
) -> Result<&'a Value, Error> {
    let mut prefix = p;
    loop {
        if let Some(v) = values.get(prefix) {
            let mut v = *v;
            if prefix != p {
                for part in p.get(prefix.len() + 1..).ok_or(Error::Syntax)?.split('.') {
                    v = match v {
                        Value::Record(fields) => fields.get(part),
                        _ => None,
                    }
                    .ok_or_else(|| Error::MissingValue(p.into()))?;
                }
            }
            if !valid(v, &check::field(p, schema, o)?.kind) {
                return Err(Error::InvalidValue(p.into()));
            }
            return Ok(v);
        }
        let Some((shorter, _)) = prefix.rsplit_once('.') else {
            break;
        };
        prefix = shorter;
    }
    Err(Error::MissingValue(p.into()))
}
fn plain(v: &Value) -> Result<String, Error> {
    match v {
        Value::Text(s) | Value::Enum(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Integer(n) => Ok(n.to_string()),
        Value::Decimal(d) => Ok(d.to_canonical_string()),
        _ => Err(Error::Type("render".into())),
    }
}
fn separators(locale: Locale) -> (&'static str, &'static str, &'static str) {
    match locale {
        Locale::Und => (".", "", ", "),
        Locale::EnUs | Locale::EnGb => (".", ",", " and "),
        Locale::DeDe => (",", ".", " und "),
        Locale::FrFr => (",", "\u{202f}", " et "),
        Locale::EsEs => (",", ".", " y "),
        Locale::PlPl => (",", "\u{a0}", " i "),
    }
}
fn number(v: &Value, scale: u32, locale: Locale) -> Result<String, Error> {
    let d = match v {
        Value::Integer(n) => Decimal::from_i64(*n),
        Value::Decimal(d) => *d,
        _ => return Err(Error::Type("number".into())),
    };
    let d = d
        .round_to_scale(scale, Rounding::HalfEven)
        .map_err(|_| Error::Arithmetic)?;
    let (decimal, group, _) = separators(locale);
    let mag = d.units().unsigned_abs();
    let digits = (mag / 1_000_000_000).to_string();
    let mut out = String::new();
    if d.is_negative() {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(group);
        }
        out.push(c);
    }
    if scale > 0 {
        out.push_str(decimal);
        let fraction = mag % 1_000_000_000 / 10u128.pow(9 - scale);
        out.push_str(&format!("{fraction:0width$}", width = scale as usize));
    }
    Ok(out)
}
fn date(t: Timestamp, iso: bool, locale: Locale) -> String {
    // Gregorian civil date from epoch days, using 400-year eras. Timestamp's
    // validated range (0 to year 9999, `Timestamp::from_ms`) keeps every
    // intermediate well inside i64 and non-negative, so truncating division
    // is floor division here.
    let z = t.as_ms() / 86_400_000 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    if iso || locale == Locale::Und {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        match locale {
            Locale::EnUs => format!("{m:02}/{d:02}/{y:04}"),
            Locale::DeDe | Locale::PlPl => format!("{d:02}.{m:02}.{y:04}"),
            _ => format!("{d:02}/{m:02}/{y:04}"),
        }
    }
}
struct Writer<'a> {
    pending_entity: String,
    o: &'a Options,
    result: Rendered,
}
impl Writer<'_> {
    fn atom(&mut self, s: &str) -> Result<(), Error> {
        if self.result.truncated {
            return Ok(());
        }
        if s.len() > self.o.max_bytes.saturating_sub(self.result.text.len()) {
            if self.o.overflow == Overflow::Refuse {
                return Err(Error::OutputBound);
            }
            self.result.truncated = true;
        } else {
            self.result.text.push_str(s);
        }
        Ok(())
    }
    fn flush_pending(&mut self, entity: bool) -> Result<(), Error> {
        let pending = std::mem::take(&mut self.pending_entity);
        if entity {
            self.atom(&pending)?;
        } else {
            for c in pending.chars() {
                self.atom(c.encode_utf8(&mut [0; 4]))?;
            }
        }
        Ok(())
    }
    fn push(&mut self, s: &str) -> Result<(), Error> {
        for c in s.chars() {
            if self.result.truncated {
                break;
            }
            if !self.pending_entity.is_empty() {
                if c == ';' && self.pending_entity.len() > 1 {
                    self.pending_entity.push(c);
                    self.flush_pending(true)?;
                    continue;
                }
                if c.is_ascii_alphanumeric() || c == '#' {
                    // One byte beyond the entire output budget is sufficient to
                    // detect overflow; retain no unbounded entity candidate.
                    if self.pending_entity.len() <= self.o.max_bytes {
                        self.pending_entity.push(c);
                    }
                    continue;
                }
                self.flush_pending(false)?;
            }
            if self.o.escape == Escape::Html && c == '&' {
                self.pending_entity.push(c);
            } else {
                self.atom(c.encode_utf8(&mut [0; 4]))?;
            }
        }
        Ok(())
    }
    fn nodes(
        &mut self,
        nodes: &[Node],
        values: &BTreeMap<String, &Value>,
        schema: &Schema,
    ) -> Result<(), Error> {
        for node in nodes {
            if self.result.truncated {
                break;
            }
            match node {
                Node::Text(s) => self.push(s)?,
                Node::Emit(p, fmt) => {
                    let v = value(p, values, schema, self.o)?;
                    self.result.reads.insert(p.clone());
                    let s = match fmt {
                        Format::Plain | Format::Raw => plain(v)?,
                        Format::Number(scale) => number(v, *scale, self.o.locale)?,
                        Format::Money(scale, currency) => {
                            let code = match currency {
                                Currency::Literal(s) => s.clone(),
                                Currency::Path(cp) => {
                                    self.result.reads.insert(cp.clone());
                                    plain(value(cp, values, schema, self.o)?)?
                                }
                            };
                            format!("{} {code}", number(v, *scale, self.o.locale)?)
                        }
                        Format::Date(iso) => match v {
                            Value::Timestamp(t) => date(*t, *iso, self.o.locale),
                            _ => return Err(Error::Type(p.clone())),
                        },
                        Format::Join => match v {
                            Value::List(items) => {
                                let mut s = String::new();
                                for (i, v) in items.iter().enumerate() {
                                    if i > 0 {
                                        s.push_str(if i + 1 == items.len() {
                                            separators(self.o.locale).2
                                        } else {
                                            ", "
                                        });
                                    }
                                    s.push_str(&plain(v)?);
                                }
                                s
                            }
                            _ => return Err(Error::Type(p.clone())),
                        },
                    };
                    self.push(&if matches!(fmt, Format::Raw) {
                        s
                    } else {
                        escaped(&s, self.o)
                    })?;
                }
                Node::Section(p, inverted, body) => {
                    let v = value(p, values, schema, self.o)?;
                    self.result.reads.insert(p.clone());
                    let active = match v {
                        Value::Bool(b) => *b,
                        Value::Maybe(v) => v.is_some(),
                        Value::List(v) => !v.is_empty(),
                        _ => return Err(Error::Type(p.clone())),
                    };
                    if *inverted {
                        if !active {
                            self.nodes(body, values, schema)?;
                        }
                        continue;
                    }
                    if !active {
                        continue;
                    }
                    let mut scoped_values = values.clone();
                    let mut scoped_schema = schema.clone();
                    match (v, &check::field(p, schema, self.o)?.kind) {
                        (Value::Maybe(Some(inner)), Type::Maybe(f)) => {
                            scoped_values.insert(p.clone(), inner);
                            scoped_schema.insert(p.clone(), *f.clone());
                            self.nodes(body, &scoped_values, &scoped_schema)?;
                        }
                        (Value::List(items), Type::List { element, .. }) => {
                            scoped_schema.insert("this".into(), *element.clone());
                            for item in items {
                                if self.result.truncated {
                                    break;
                                }
                                scoped_values.insert("this".into(), item);
                                self.nodes(body, &scoped_values, &scoped_schema)?;
                            }
                        }
                        _ => self.nodes(body, values, schema)?,
                    }
                }
            }
        }
        Ok(())
    }
}
pub(super) fn render(t: &Template, values: &Values) -> Result<Rendered, Error> {
    let values = values.iter().map(|(k, v)| (k.clone(), v)).collect();
    let mut w = Writer {
        pending_entity: String::new(),
        o: &t.options,
        result: Rendered {
            text: String::new(),
            truncated: false,
            reads: BTreeSet::new(),
        },
    };
    w.nodes(&t.nodes, &values, &t.schema)?;
    w.flush_pending(false)?;
    if t.options.required && w.result.text.is_empty() {
        return Err(Error::MayBeEmpty);
    }
    Ok(w.result)
}
