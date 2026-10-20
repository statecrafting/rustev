//! `rustev calibrate fit | qualify` (spec 006, 3.10), rendered by
//! `rustev-surface`.

use rustev_surface::calibrate::{FitArgs, QualifyArgs};

use crate::args::{Parsed, Usage};
use crate::eval::split;
use crate::io::Reads;
use crate::out::Res;
use crate::replay::{replay_config, resolver};

pub fn fit(p: &Parsed) -> Result<Res, Usage> {
    let replay = replay_config(p)?;
    let reads = Reads::default();
    let resolver = resolver(p);
    Ok(rustev_surface::calibrate::fit(
        &p.name(),
        &reads,
        FitArgs {
            replay: &replay,
            out: p.req("out"),
            dataset: p.req("dataset"),
            config: p.req("config"),
            bundles: p.req("bundles"),
            step: p.req("step"),
        },
        resolver.as_ref(),
    ))
}

pub fn qualify_cmd(p: &Parsed) -> Result<Res, Usage> {
    let split = split(p)?;
    let reads = Reads::default();
    Ok(rustev_surface::calibrate::qualify_cmd(
        &p.name(),
        &reads,
        QualifyArgs {
            split,
            calibration: p.req("calibration"),
            fit: p.one("fit"),
            dataset: p.req("dataset"),
        },
    ))
}
