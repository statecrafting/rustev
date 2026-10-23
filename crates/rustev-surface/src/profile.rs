//! Parameterized commands (spec 034). All reads share the host's budget.
use crate::{
    deps::{self, DepArgs, Deps, RulesHook},
    host::Host,
    out::{Done, Out, Res, code},
    plan,
};
use rustev_contract::{
    Document,
    execution::ExecutionPolicy,
    limits::{DEFINITION_V1, DESCRIPTOR_V1},
};
use rustev_core::{
    Compiled,
    instantiate::{
        InstantiationRefusal, ProfiledRefusal, compile_profiled_with, instantiate, select_template,
    },
};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub struct ProfileArgs<'a> {
    pub definitions: &'a [String],
    pub profile: Option<&'a str>,
    pub bases: &'a [String],
    pub variant: Option<&'a str>,
    pub pin: Option<&'a str>,
}

pub(crate) struct Inputs {
    definitions: Vec<Vec<u8>>,
    profile: Option<Vec<u8>>,
    bases: Vec<Vec<u8>>,
}
impl Inputs {
    pub(crate) fn read(
        command: &str,
        host: &dyn Host,
        args: ProfileArgs<'_>,
    ) -> Result<Self, Done> {
        let read = |path: &str, limit| host.read(path, limit).map_err(|e| Done::io(command, e));
        Ok(Self {
            definitions: args
                .definitions
                .iter()
                .map(|p| read(p, DEFINITION_V1.max_bytes))
                .collect::<Result<_, _>>()?,
            profile: args
                .profile
                .map(|p| read(p, DESCRIPTOR_V1.max_bytes))
                .transpose()?,
            bases: args
                .bases
                .iter()
                .map(|p| read(p, DESCRIPTOR_V1.max_bytes))
                .collect::<Result<_, _>>()?,
        })
    }
    fn definitions(&self) -> Vec<&[u8]> {
        self.definitions.iter().map(Vec::as_slice).collect()
    }
    fn bases(&self) -> Vec<&[u8]> {
        self.bases.iter().map(Vec::as_slice).collect()
    }
    pub(crate) fn compile(
        &self,
        args: ProfileArgs<'_>,
        deps: &Deps,
        execution: Option<&ExecutionPolicy>,
    ) -> Result<Compiled, ProfiledRefusal> {
        compile_profiled_with(
            &self.definitions(),
            self.profile.as_deref(),
            &self.bases(),
            args.variant,
            args.pin,
            &deps.descriptors,
            &deps.calibrations,
            &[],
            execution,
        )
    }
}

pub fn refused(command: &str, e: &InstantiationRefusal) -> Done {
    Done::new(
        code::REFUSED,
        Out::new(command, "refused")
            .set("stage", "instantiate")
            .set("code", e.code.as_str())
            .set("subject", &e.subject)
            .set("detail", &e.detail),
    )
}
pub(crate) fn compile_refused(command: &str, e: &ProfiledRefusal) -> Done {
    match e {
        ProfiledRefusal::Instantiate(e) => refused(command, e),
        ProfiledRefusal::Compile {
            refusal,
            parameters,
        } => {
            let mut d = plan::refused(command, refusal);
            d.out = d.out.set("stage", "compile").set("parameters", parameters);
            d
        }
    }
}
pub(crate) fn execution(
    command: &str,
    host: &dyn Host,
    path: Option<&str>,
) -> Result<Option<ExecutionPolicy>, Done> {
    path.map(|p| {
        let bytes = host
            .read(p, DESCRIPTOR_V1.max_bytes)
            .map_err(|e| Done::io(command, e))?;
        ExecutionPolicy::parse(&bytes).map_err(|e| {
            plan::dep_failed(
                command,
                deps::DepFail::Parse {
                    subject: format!("execution:{p}"),
                    detail: e.to_string(),
                },
            )
        })
    })
    .transpose()
}

/// The core report is flattened into the command envelope without changing
/// its numeric pruning-stage metadata (3.13.3).
pub fn check(command: &str, host: &dyn Host, args: ProfileArgs<'_>) -> Res {
    let i = Inputs::read(command, host, args)?;
    let report = rustev_core::profile_check(
        &i.definitions(),
        i.profile.as_deref(),
        &i.bases(),
        args.variant,
        args.pin,
    )
    .map_err(|e| refused(command, &e))?;
    let failed = report.is_refused();
    let mut out = Out::new(command, if failed { "refused" } else { "ok" });
    for (key, value) in serde_json::to_value(report)
        .expect("report serializes")
        .as_object()
        .expect("report object")
    {
        out = out.set(key, value);
    }
    Ok(Done::new(
        if failed { code::REFUSED } else { code::OK },
        out,
    ))
}

/// Additive entry point: existing binding signatures remain intact.
pub fn plan_command(
    command: &str,
    host: &dyn Host,
    args: ProfileArgs<'_>,
    dep_args: DepArgs<'_>,
    execution_path: Option<&str>,
    rules: &mut RulesHook<'_>,
    output: Option<&str>,
) -> Res {
    if let Some(out) = output {
        host.ensure_absent(out).map_err(|e| Done::io(command, e))?;
    }
    let i = Inputs::read(command, host, args)?;
    // Preserve the old parser, compiler and output without new flags for
    // concrete documents, including spec 033's v2 actionability definitions.
    // A v2 template opts in through parameterization members. This conservative
    // distinction preserves existing v3 identities and their CLI goldens.
    let new_flags = args.profile.is_some()
        || !args.bases.is_empty()
        || args.variant.is_some()
        || args.pin.is_some()
        || args.definitions.len() != 1;
    let template = i.definitions.first().is_some_and(|bytes| {
        serde_json::from_slice::<Value>(bytes)
            .ok()
            .is_some_and(|v| {
                v["schema"] == "rustev.definition/2"
                    && (v.get("parameters").is_some()
                        || v.get("variants").is_some()
                        || rustev_contract::definition::Definition::parse(bytes).is_err())
            })
    });
    if !new_flags && !template {
        let c = plan::compile_definition_bytes(
            command,
            host,
            plan::CompileArgs {
                definition: &args.definitions[0],
                deps: dep_args,
                execution: execution_path,
            },
            rules,
            &i.definitions[0],
        )?;
        return match output {
            Some(path) => plan::publish_compiled(command, host, &c, path),
            None => Ok(plan::checked(command, &c)),
        };
    }

    let selection = select_template(&i.definitions(), i.profile.as_deref(), args.pin)
        .map_err(|e| refused(command, &e))?;
    let names = selection.template.json["variants"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Refuse a missing compile variant before reading dependencies.
    if output.is_some()
        && args.variant.is_none()
        && !names.is_empty()
        && let Err(e) = instantiate(
            &i.definitions(),
            i.profile.as_deref(),
            &i.bases(),
            None,
            args.pin,
        )
    {
        return Err(refused(command, &e));
    }
    let deps = deps::load(host, dep_args, rules).map_err(|e| plan::dep_failed(command, e))?;
    let execution = execution(command, host, execution_path)?;
    if output.is_none() && args.variant.is_none() && !names.is_empty() {
        let mut variants = vec![];
        let mut first = None;
        for name in names {
            // Variant names were validated as strings during selection.
            let Some(name) = name.as_str() else { continue };
            let selected = ProfileArgs {
                variant: Some(name),
                ..args
            };
            variants.push(match i.compile(selected, &deps, execution.as_ref()) {
                Ok(c) => match instantiate(&i.definitions(), i.profile.as_deref(), &i.bases(), Some(name), args.pin) {
                    Ok(inst) => json!({"name":name,"result":"ok","plan_id":c.id,"pruned":inst.pruned}),
                    Err(r) => {
                        first.get_or_insert_with(|| name.to_owned());
                        json!({"name":name,"result":"refused","stage":"instantiate","code":r.code.as_str(),"subject":&r.subject,"detail":&r.detail})
                    }
                },
                Err(e) => {
                    first.get_or_insert_with(|| name.to_owned());
                    // Compile category's existing snake-case name becomes code;
                    // subject and detail are copied verbatim. No shortfalls here.
                    let (stage, code, subject, detail) = match &e {
                        ProfiledRefusal::Instantiate(r) => ("instantiate", r.code.as_str(), &r.subject, &r.detail),
                        ProfiledRefusal::Compile { refusal: r, .. } => ("compile", plan::category_name(r.category), &r.subject, &r.detail),
                    };
                    json!({"name":name,"result":"refused","stage":stage,"code":code,"subject":subject,"detail":detail})
                }
            });
        }
        let mut out = Out::new(command, if first.is_some() { "refused" } else { "ok" })
            .set("variants", &variants);
        if let Some(name) = &first {
            out = out.set("first_refused", name);
        }
        return Ok(Done::new(
            if first.is_some() {
                code::REFUSED
            } else {
                code::OK
            },
            out,
        ));
    }
    let c = i
        .compile(args, &deps, execution.as_ref())
        .map_err(|e| compile_refused(command, &e))?;
    let mut out = Out::new(command, if output.is_some() { "compiled" } else { "ok" })
        .set("plan_id", &c.id)
        .set("plan_schema", &c.plan.schema)
        .set("definition_id", &selection.template.id)
        .set(
            "definition_version",
            &selection.template.version.to_string(),
        )
        .set(
            "variant",
            &c.plan
                .instantiation
                .as_ref()
                .map(|i| &i.variant)
                .unwrap_or(&rustev_contract::plan::Variant::None),
        )
        .set(
            "chain",
            &c.plan
                .instantiation
                .as_ref()
                .map(|i| &i.profile)
                .unwrap_or(&rustev_contract::plan::ProfileChain::None),
        );
    if let Some(path) = output {
        host.create(path, &c.canonical())
            .map_err(|e| Done::io(command, e))?;
        out = out.set("out", path);
    }
    Ok(Done::ok(out))
}
