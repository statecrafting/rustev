//! The argument grammar (spec 006, 3.1.1): a command path, then long flags.
//! Every flag a command accepts is declared with its arity; anything else,
//! a missing value, a repeated single flag or a missing required flag is a
//! usage error found before any file is read.

use std::collections::BTreeMap;

/// How often a flag may occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// At most once.
    One,
    /// Any number of times.
    Many,
}

/// One command: its path, its flags and which of them are required.
#[derive(Debug)]
pub struct CommandSpec {
    pub path: &'static [&'static str],
    pub flags: &'static [(&'static str, Arity)],
    pub required: &'static [&'static str],
}

macro_rules! flags {
    ($($name:literal: $arity:ident),*) => {
        &[$(($name, Arity::$arity)),*]
    };
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        path: &["plan", "check"],
        flags: flags!("definition": One, "descriptor": Many, "rules": Many,
                      "calibration": Many, "execution": One),
        required: &["definition"],
    },
    CommandSpec {
        path: &["plan", "compile"],
        flags: flags!("definition": One, "descriptor": Many, "rules": Many,
                      "calibration": Many, "execution": One, "out": One),
        required: &["definition", "out"],
    },
    CommandSpec {
        path: &["plan", "show"],
        flags: flags!("plan": One, "descriptor": Many, "rules": Many, "calibration": Many),
        required: &["plan"],
    },
    CommandSpec {
        path: &["run"],
        flags: flags!("plan": One, "rules": Many, "calibration": Many, "snapshot": One,
                      "evaluation-time": One, "decision-id": One, "record-out": One,
                      "deadline-ms": One, "max-parallel-requests": One,
                      "sink-timeout-ms": One, "capture-bytes": One, "bundle-out": One,
                      "now-ms": One, "retain": One, "store": One, "lifetime-ms": One,
                      "host-cap-ms": One, "scope": One, "tenant": One,
                      "context-revision": One, "principal-scope": One),
        required: &[
            "plan",
            "rules",
            "snapshot",
            "evaluation-time",
            "decision-id",
            "record-out",
        ],
    },
    CommandSpec {
        path: &["replay"],
        flags: flags!("bundle": One, "now-ms": One, "store": One, "host-cap-ms": One,
                      "scope": One, "tenant": One, "context-revision": One,
                      "principal-scope": One),
        required: &["bundle", "now-ms", "tenant", "context-revision"],
    },
    CommandSpec {
        path: &["eval"],
        flags: flags!("dataset": One, "split": One, "config": One, "adapter": One,
                      "bundles": One, "now-ms": One, "store": One, "host-cap-ms": One,
                      "out": One, "candidate-plan": One, "descriptor": Many,
                      "rules": Many, "calibration": Many, "scope": One, "tenant": One,
                      "context-revision": One, "principal-scope": One),
        required: &[
            "dataset",
            "split",
            "config",
            "adapter",
            "bundles",
            "now-ms",
            "out",
            "tenant",
            "context-revision",
        ],
    },
    CommandSpec {
        path: &["gate"],
        flags: flags!("baseline": One, "candidate": One, "gate": One),
        required: &["baseline", "candidate", "gate"],
    },
    CommandSpec {
        path: &["calibrate", "fit"],
        flags: flags!("dataset": One, "config": One, "step": One, "bundles": One,
                      "now-ms": One, "store": One, "host-cap-ms": One, "out": One,
                      "scope": One, "tenant": One, "context-revision": One,
                      "principal-scope": One),
        required: &[
            "dataset",
            "config",
            "step",
            "bundles",
            "now-ms",
            "out",
            "tenant",
            "context-revision",
        ],
    },
    CommandSpec {
        path: &["calibrate", "qualify"],
        flags: flags!("calibration": One, "fit": One, "dataset": One, "split": One),
        required: &["calibration", "dataset", "split"],
    },
];

/// A usage error: reported on stderr, exit 2, nothing read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage(pub String);

pub fn usage<T>(msg: impl Into<String>) -> Result<T, Usage> {
    Err(Usage(msg.into()))
}

/// What was asked for.
#[derive(Debug)]
pub enum Invocation {
    Help,
    Command(Parsed),
}

/// A command with its flag values, in the order given.
#[derive(Debug)]
pub struct Parsed {
    pub spec: &'static CommandSpec,
    values: BTreeMap<&'static str, Vec<String>>,
}

impl Parsed {
    /// The command path joined by spaces, as the output names it.
    pub fn name(&self) -> String {
        self.spec.path.join(" ")
    }

    pub fn one(&self, flag: &str) -> Option<&str> {
        self.values
            .get(flag)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    pub fn many(&self, flag: &str) -> &[String] {
        self.values.get(flag).map_or(&[], Vec::as_slice)
    }

    pub fn has(&self, flag: &str) -> bool {
        self.values.contains_key(flag)
    }

    /// A required flag's value; the parser has already checked presence.
    pub fn req(&self, flag: &str) -> &str {
        self.one(flag).unwrap_or_default()
    }

    /// An optional unsigned integer flag within `[min, max]`.
    pub fn uint(&self, flag: &str, min: u64, max: u64) -> Result<Option<u64>, Usage> {
        let Some(s) = self.one(flag) else {
            return Ok(None);
        };
        match uint(s) {
            Some(v) if (min..=max).contains(&v) => Ok(Some(v)),
            _ => usage(format!(
                "--{flag} takes an unsigned decimal integer in {min}..={max}, not {s:?}"
            )),
        }
    }

    /// Refuse each of `flags` that was given, naming `why`.
    pub fn forbid(&self, flags: &[&str], why: &str) -> Result<(), Usage> {
        match flags.iter().find(|f| self.has(f)) {
            Some(f) => usage(format!("--{f} {why}")),
            None => Ok(()),
        }
    }

    /// Require each of `flags`, naming `why`.
    pub fn need(&self, flags: &[&str], why: &str) -> Result<(), Usage> {
        match flags.iter().find(|f| !self.has(f)) {
            Some(f) => usage(format!("--{f} is required {why}")),
            None => Ok(()),
        }
    }
}

/// Unsigned decimal ASCII: no sign, no leading zeros except `0`, within
/// `u64`.
pub fn uint(s: &str) -> Option<u64> {
    let ok =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if ok { s.parse().ok() } else { None }
}

/// Parse the arguments after the program name.
pub fn parse(argv: &[String]) -> Result<Invocation, Usage> {
    if argv.is_empty() {
        return usage("no command; run `rustev help`");
    }
    if argv[0] == "help" || argv[0] == "--help" {
        return Ok(Invocation::Help);
    }
    let Some(spec) = COMMANDS
        .iter()
        .filter(|c| c.path.len() <= argv.len() && c.path.iter().zip(argv).all(|(p, a)| p == a))
        .max_by_key(|c| c.path.len())
    else {
        // `rustev plan --help`: help where a subcommand was expected.
        if argv.get(1).is_some_and(|a| a == "--help") {
            return Ok(Invocation::Help);
        }
        return usage(format!("unknown command {:?}", argv[0]));
    };
    let mut values: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut rest = argv[spec.path.len()..].iter();
    while let Some(arg) = rest.next() {
        let Some(name) = arg.strip_prefix("--") else {
            return usage(format!("unexpected argument {arg:?}"));
        };
        // Help only where a flag name is expected, never as a value.
        if name == "help" {
            return Ok(Invocation::Help);
        }
        let Some(&(flag, arity)) = spec.flags.iter().find(|(f, _)| *f == name) else {
            return usage(format!("{} takes no flag --{name}", spec.path.join(" ")));
        };
        // A value never starts with `--`: that is the next flag, and the
        // value is missing.
        let Some(value) = rest.next().filter(|v| !v.starts_with("--")) else {
            return usage(format!("--{flag} needs a value"));
        };
        let slot = values.entry(flag).or_default();
        if arity == Arity::One && !slot.is_empty() {
            return usage(format!("--{flag} may be given once"));
        }
        slot.push(value.clone());
    }
    if let Some(f) = spec.required.iter().find(|f| !values.contains_key(*f)) {
        return usage(format!("--{f} is required"));
    }
    Ok(Invocation::Command(Parsed { spec, values }))
}

pub const HELP: &str = "\
rustev: compile, inspect, run, replay and evaluate Rustev decision plans.

  rustev plan check   --definition F [--descriptor F]... [--rules F]... [--calibration F]... [--execution F]
  rustev plan compile --definition F [...same...] --out F
  rustev plan show    --plan F [--descriptor F]... [--rules F]... [--calibration F]...
  rustev run          --plan F --rules F... [--calibration F]... --snapshot F
                      --evaluation-time MS --decision-id ID --record-out F
                      [--deadline-ms N] [--max-parallel-requests N] [--sink-timeout-ms N]
                      [--capture-bytes N --bundle-out F --now-ms MS|system SCOPE
                       [--retain digest-only|embedded|external] [--store DIR]
                       [--lifetime-ms N] [--host-cap-ms N]]
  rustev replay       --bundle F --now-ms MS|system SCOPE [--store DIR] [--host-cap-ms N]
  rustev eval         --dataset F --split S --config F --adapter F --bundles DIR
                      --now-ms MS|system SCOPE --out DIR [--store DIR] [--host-cap-ms N]
                      [--candidate-plan F [--descriptor F]... [--rules F]... [--calibration F]...]
  rustev gate         --baseline DIR --candidate DIR --gate NAME
  rustev calibrate fit     --dataset F --config F --step ID --bundles DIR
                           --now-ms MS|system SCOPE --out DIR [--store DIR] [--host-cap-ms N]
  rustev calibrate qualify --calibration F [--fit F] --dataset F --split S

  SCOPE: [--scope principal|tenant-only] --tenant H --context-revision H
         [--principal-scope H]   (required in principal mode, the default)

Each command prints one JSON object on stdout; the exit code is its status.
";
