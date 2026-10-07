// SPDX-License-Identifier: Apache-2.0
//! Prints `buildstamp.tex`: a `\buildstamp` macro naming this
//! repository's commit, TxHDL's commit and the day of the build, read
//! from Bazel's workspace status files.
//!
//! Usage: `stamp <stable-status.txt> <volatile-status.txt>`.
use std::collections::BTreeMap;
use std::fs;

fn status(p: &str) -> BTreeMap<String, String> {
    fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The date of a Unix time, as YYYY-MM-DD, in UTC.
fn date(secs: i64) -> String {
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 3 {
        eprintln!("usage: stamp <stable-status.txt> <volatile-status.txt>");
        std::process::exit(2);
    }
    let s = status(&a[1]);
    let v = status(&a[2]);
    let short = |k: &str| {
        s.get(k)
            .map(|c| c.chars().take(12).collect::<String>())
            .unwrap_or_else(|| "unknown".into())
    };
    let day = v
        .get("BUILD_TIMESTAMP")
        .and_then(|t| t.parse().ok())
        .map(date)
        .unwrap_or_else(|| "an unknown day".into());
    println!(
        "\\newcommand{{\\buildstamp}}{{Built by Bazel from commit \\code{{{}}} of \
         \\code{{vreteno-conformance}}, with TxHDL commit \\code{{{}}}, on \\mbox{{{}}}.}}",
        short("STABLE_GIT_COMMIT"),
        short("STABLE_TXHDL_COMMIT"),
        day
    );
}
