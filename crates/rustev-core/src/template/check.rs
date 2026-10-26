use super::*;
pub(super) fn options(o: &Options, schema: &Schema) -> Result<(), Error> {
    if !(1..=16384).contains(&o.max_bytes) {
        return Err(Error::OutputBound);
    }
    // Spec 037 3.4.7 sets this floor for every required truncating field,
    // whatever the escape mode: truncation keeps a first character or entity.
    if o.required && o.overflow == Overflow::Truncate && o.max_bytes < 8 {
        return Err(Error::OutputBound);
    }
    if let Mode::Backstop { bind: Some(bind) } = &o.mode
        && (!parse::name(bind)
            || ["input", "step", "proposal", "param", "this"].contains(&bind.as_str()))
    {
        return Err(Error::InvalidBind);
    }
    if o.mode == Mode::Prompt {
        if o != &Options::prompt() {
            return Err(Error::InvalidDeclaration);
        }
        let params = match schema.get("param") {
            Some(Field {
                kind: Type::Record(p),
                ..
            }) => p,
            None if schema.is_empty() => return Ok(()),
            _ => return Err(Error::InvalidDeclaration),
        };
        if schema.len() != 1 || params.len() > 32 {
            return Err(Error::InvalidDeclaration);
        }
        for (name, field) in params {
            if !parse::name(name)
                || !matches!(
                    &field.kind,
                    Type::Bool
                        | Type::Integer
                        | Type::Decimal
                        | Type::Enum(_)
                        | Type::Text {
                            max_bytes: 0..=1024
                        }
                )
            {
                return Err(Error::InvalidDeclaration);
            }
        }
    }
    if schema
        .keys()
        .any(|root| !parse::name(root) || root == "this")
    {
        return Err(Error::InvalidDeclaration);
    }
    for field in schema.values() {
        declaration(&field.kind, 0)?;
    }
    Ok(())
}
fn declaration(t: &Type, depth: usize) -> Result<(), Error> {
    if depth > 64 {
        return Err(Error::InvalidDeclaration);
    }
    match t {
        Type::Enum(opts)
            if opts.is_empty() || opts.iter().collect::<BTreeSet<_>>().len() != opts.len() =>
        {
            return Err(Error::InvalidDeclaration);
        }
        Type::Maybe(f) | Type::List { element: f, .. } => declaration(&f.kind, depth + 1)?,
        Type::Record(fields) => {
            for f in fields.values() {
                declaration(&f.kind, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn class(f: &Field, p: &str) -> Result<(), Error> {
    match f.class {
        Class::Exact => Ok(()),
        Class::Generated => Err(Error::ModelOutputFlow(p.into())),
        _ => Err(Error::NotExact(p.into())),
    }
}
pub(super) fn field<'a>(p: &str, schema: &'a Schema, o: &Options) -> Result<&'a Field, Error> {
    let root = p.split('.').next().ok_or(Error::Syntax)?;
    let allowed = match &o.mode {
        Mode::Prompt => root == "param",
        Mode::Backstop { bind } => {
            ["input", "step", "proposal", "this"].contains(&root) || bind.as_deref() == Some(root)
        }
    };
    if !allowed || (!p.contains('.') && root != "this") {
        return Err(Error::UnknownPath(p.into()));
    }
    // Scoped exact-path overrides unwrap maybe values without a context stack.
    let mut prefix = p;
    loop {
        if let Some(f) = schema.get(prefix) {
            class(f, p)?;
            let mut f = f;
            if prefix != p {
                let suffix = p.get(prefix.len() + 1..).ok_or(Error::Syntax)?;
                for part in suffix.split('.') {
                    f = match &f.kind {
                        Type::Record(fields) => fields.get(part),
                        _ => None,
                    }
                    .ok_or_else(|| Error::UnknownPath(p.into()))?;
                    class(f, p)?;
                }
            }
            return Ok(f);
        }
        let Some((shorter, _)) = prefix.rsplit_once('.') else {
            break;
        };
        prefix = shorter;
    }
    Err(Error::UnknownPath(p.into()))
}
fn add(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b).ok_or(Error::OutputBound)
}
fn mul(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or(Error::OutputBound)
}
fn scalar(t: &Type, raw: bool, o: &Options) -> Result<Bounds, Error> {
    let (min, max) = match t {
        Type::Bool if raw => (4, 5),
        Type::Enum(opts) => {
            if raw
                && opts.iter().any(|s| {
                    s.chars().any(|c| {
                        c.is_control() || (o.escape == Escape::Html && "&<>\"'".contains(c))
                    })
                })
            {
                return Err(Error::UnsafeRaw);
            }
            (
                opts.iter()
                    .map(String::len)
                    .min()
                    .ok_or(Error::InvalidDeclaration)?,
                opts.iter()
                    .map(String::len)
                    .max()
                    .ok_or(Error::InvalidDeclaration)?,
            )
        }
        Type::Text { max_bytes } if !raw => (0, *max_bytes),
        Type::Integer if !raw => (1, 20),
        Type::Decimal if !raw => (1, 41),
        _ => return Err(Error::Type("interpolation".into())),
    };
    // C1 controls shrink from two UTF-8 bytes to one space. For enum minima
    // measure the actual escaped options, rather than assuming byte preservation.
    let min = if !raw {
        if let Type::Enum(opts) = t {
            opts.iter()
                .map(|s| render::escaped(s, o).len())
                .min()
                .ok_or(Error::InvalidDeclaration)?
        } else {
            min
        }
    } else {
        min
    };
    Ok(Bounds {
        min,
        max: mul(
            max,
            if !raw && o.escape == Escape::Html {
                6
            } else {
                1
            },
        )?,
    })
}
pub(super) fn nodes(
    ns: &[Node],
    schema: &Schema,
    o: &Options,
    refs: &mut BTreeSet<String>,
) -> Result<Bounds, Error> {
    let mut total = Bounds { min: 0, max: 0 };
    let mut sections: BTreeMap<String, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for n in ns {
        let b = match n {
            Node::Text(s) => {
                // An authored entity can exceed the six-byte escaped entities.
                // Refuse a potentially empty required truncated prefix, including
                // an entity that could continue across subsequent template nodes.
                if o.required
                    && o.overflow == Overflow::Truncate
                    && o.escape == Escape::Html
                    && total.min == 0
                    && let Some(tail) = s.strip_prefix('&')
                {
                    let candidate: String = tail
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '#')
                        .collect();
                    let next = tail.get(candidate.len()..).ok_or(Error::Syntax)?;
                    if next.is_empty()
                        || (next.starts_with(';') && candidate.len() + 2 > o.max_bytes)
                    {
                        return Err(Error::MayBeEmpty);
                    }
                }
                Bounds {
                    min: s.len(),
                    max: s.len(),
                }
            }
            Node::Emit(p, fmt) => {
                refs.insert(p.clone());
                let f = field(p, schema, o)?;
                match fmt {
                    Format::Plain => scalar(&f.kind, false, o)?,
                    Format::Raw => scalar(&f.kind, true, o)?,
                    _ => {
                        let (min, max) = match (fmt, &f.kind) {
                            (Format::Number(scale), Type::Integer | Type::Decimal) => {
                                (1, number_bound(&f.kind, *scale, o.locale))
                            }
                            (Format::Money(scale, currency), Type::Integer | Type::Decimal) => {
                                if let Currency::Path(cp) = currency {
                                    refs.insert(cp.clone());
                                    match &field(cp, schema, o)?.kind {
                                        Type::Enum(opts) if opts.iter().all(|s| parse::code(s)) => {
                                        }
                                        _ => return Err(Error::InvalidCurrency),
                                    }
                                }
                                (1, number_bound(&f.kind, *scale, o.locale) + 4)
                            }
                            (Format::Date(_), Type::Timestamp) => (1, 10),
                            (Format::Join, Type::List { element, max_items }) => {
                                class(element, p)?;
                                if !matches!(element.kind, Type::Text { .. } | Type::Enum(_)) {
                                    return Err(Error::Type(p.clone()));
                                }
                                let b = scalar(
                                    &element.kind,
                                    false,
                                    &Options {
                                        escape: Escape::Text,
                                        ..o.clone()
                                    },
                                )?;
                                (
                                    0,
                                    add(
                                        mul(b.max, *max_items)?,
                                        mul(max_items.saturating_sub(1), 5)?,
                                    )?,
                                )
                            }
                            _ => return Err(Error::Type(p.clone())),
                        };
                        Bounds {
                            min,
                            max: mul(max, if o.escape == Escape::Html { 6 } else { 1 })?,
                        }
                    }
                }
            }
            Node::Section(p, inverted, body) => {
                refs.insert(p.clone());
                let f = field(p, schema, o)?;
                let mut scoped = schema.clone();
                let count = match &f.kind {
                    Type::Bool => 1,
                    Type::Maybe(inner) => {
                        class(inner, p)?;
                        if !inverted {
                            scoped.insert(p.clone(), *inner.clone());
                        }
                        1
                    }
                    Type::List { element, max_items } => {
                        class(element, p)?;
                        if !inverted {
                            scoped.insert("this".into(), *element.clone());
                        }
                        if *inverted { 1 } else { *max_items }
                    }
                    _ => return Err(Error::Type(p.clone())),
                };
                let b = nodes(body, &scoped, o, refs)?;
                let pair = sections.entry(p.clone()).or_default();
                // A statically empty list cannot provide a nonempty positive arm.
                let min = if count == 0 { 0 } else { b.min };
                if *inverted {
                    pair.1.push(min);
                } else {
                    pair.0.push(min);
                }
                // Spec 037 3.4.7 pairs a section with its inverted section for
                // the minimum only; the worst case adds every section's maximum,
                // so `refuse` stays conservative.
                Bounds {
                    min: 0,
                    max: mul(b.max, count)?,
                }
            }
        };
        total.min = add(total.min, b.min)?;
        total.max = add(total.max, b.max)?;
    }
    for (positive, negative) in sections.values() {
        for (a, b) in positive.iter().zip(negative) {
            total.min = add(total.min, *a.min(b))?;
        }
    }
    Ok(total)
}

fn number_bound(t: &Type, scale: u32, locale: Locale) -> usize {
    let digits = if matches!(t, Type::Integer) { 19 } else { 30 };
    let group_bytes = match locale {
        Locale::Und => 0,
        Locale::FrFr => 3,
        Locale::PlPl => 2,
        _ => 1,
    };
    1 + digits + ((digits - 1) / 3) * group_bytes + if scale == 0 { 0 } else { 1 + scale as usize }
}
