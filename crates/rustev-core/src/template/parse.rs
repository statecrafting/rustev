use super::*;
pub(super) fn name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn path(s: &str) -> Result<String, Error> {
    if s.split('.').all(name) {
        Ok(s.into())
    } else {
        Err(Error::Syntax)
    }
}
pub(super) fn parse(source: &str) -> Result<Vec<Node>, Error> {
    let mut remaining = source;
    body(&mut remaining, None, 0)
}
fn body(rest: &mut &str, close: Option<&str>, depth: usize) -> Result<Vec<Node>, Error> {
    let mut nodes = Vec::new();
    while !rest.is_empty() {
        let Some((literal, tail)) = rest.split_once("{{") else {
            nodes.push(Node::Text((*rest).into()));
            *rest = "";
            break;
        };
        if !literal.is_empty() {
            nodes.push(Node::Text(literal.into()));
        }
        let raw = tail.starts_with('{');
        let tail = if raw {
            tail.strip_prefix('{').ok_or(Error::Syntax)?
        } else {
            tail
        };
        let (tag, next) = tail
            .split_once(if raw { "}}}" } else { "}}" })
            .ok_or(Error::Syntax)?;
        *rest = next;
        if !raw && tag.starts_with('!') {
            continue;
        }
        if !raw && let Some(p) = tag.strip_prefix('/') {
            let p = path(p.trim_matches(' '))?;
            return if close == Some(p.as_str()) {
                Ok(nodes)
            } else {
                Err(Error::Syntax)
            };
        }
        if !raw && (tag.starts_with('#') || tag.starts_with('^')) {
            if depth == 4 {
                return Err(Error::Nesting);
            }
            let inverted = tag.starts_with('^');
            let p = path(tag.get(1..).ok_or(Error::Syntax)?.trim_matches(' '))?;
            let children = body(rest, Some(&p), depth + 1)?;
            nodes.push(Node::Section(p, inverted, children));
            continue;
        }
        let tag = tag.trim_matches(' ');
        if raw {
            nodes.push(Node::Emit(path(tag)?, Format::Raw));
            continue;
        }
        let words: Vec<_> = tag.split(' ').filter(|s| !s.is_empty()).collect();
        let first = *words.first().ok_or(Error::Syntax)?;
        if words.len() == 1 {
            nodes.push(Node::Emit(path(first)?, Format::Plain));
            continue;
        }
        let p = path(words.get(1).ok_or(Error::Syntax)?)?;
        let mut args = BTreeMap::new();
        for word in words.iter().skip(2) {
            let (k, v) = word.split_once('=').ok_or(Error::Syntax)?;
            if args.insert(k, v).is_some() {
                return Err(Error::Syntax);
            }
        }
        let scale = |args: &BTreeMap<&str, &str>| -> Result<u32, Error> {
            let s = args.get("scale").ok_or(Error::Syntax)?;
            if s.len() != 1 {
                return Err(Error::Syntax);
            }
            s.parse::<u32>()
                .ok()
                .filter(|n| *n <= 9)
                .ok_or(Error::Syntax)
        };
        let format = match first {
            "number" if args.len() == 1 => Format::Number(scale(&args)?),
            "money" if args.len() == 2 => {
                let currency = *args.get("currency").ok_or(Error::Syntax)?;
                let currency = if code(currency) {
                    Currency::Literal(currency.into())
                } else {
                    Currency::Path(path(currency)?)
                };
                Format::Money(scale(&args)?, currency)
            }
            "date" if args.len() == 1 => Format::Date(match args.get("style") {
                Some(&"iso") => true,
                Some(&"numeric") => false,
                _ => return Err(Error::Syntax),
            }),
            "list_join" if args.is_empty() && words.len() == 2 => Format::Join,
            _ => return Err(Error::Syntax),
        };
        nodes.push(Node::Emit(p, format));
    }
    if close.is_some() {
        Err(Error::Syntax)
    } else {
        Ok(nodes)
    }
}
pub(super) fn code(s: &str) -> bool {
    s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase())
}
