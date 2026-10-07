// SPDX-License-Identifier: Apache-2.0
//! Writes the conformance report's data: the tables and numbers the
//! article includes, a standalone HTML page, and a TSV of every run.
//!
//! The rule `act_report` in `//act:defs.bzl` runs this program. It
//! reads a manifest of the tests and the result files the runs wrote,
//! so every number in the article and on the page comes from a run
//! the build made.
//!
//! Usage:
//!   report --manifest <tsv> --stable <file> --volatile <file>
//!          --gcc <path> --sail <path> --act-commit <sha>
//!          --catalog <n> --extensions <a,b,c> [--excluded <suite=why>]...
//!          --out-dir <dir>
//!
//! The manifest has one line per test, with tab-separated fields:
//!   suite, name, RTL result file, Sail result file,
//!   expected-results file or `-`, issue URL or `-`.
//!
//! The outputs, all in `--out-dir`:
//!   `results.tex`  macros: counts, versions, commits, the date;
//!   `suites.tex`   the table of suites;
//!   `tests.tex`    the per-test tables, as floats;
//!   `failures.tex` a subsection per failing test, with its console;
//!   `cpichart.tex` a TikZ bar chart of cycles per instruction by suite;
//!   `report.html`  the page for the web;
//!   `results.tsv`  one line per test.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

/// One test's run on one simulator, from its result file.
#[derive(Default, Clone)]
struct Run {
    status: String,
    cycles: u64,
    retired: u64,
    console: String,
}

/// One test, with both of its runs.
struct Test {
    suite: String,
    name: String,
    rtl: Run,
    sail: Run,
    /// The values the test checks against Sail's: the words of its
    /// expected results, less the fill and the canaries.
    checks: usize,
    issue: Option<String>,
}

impl Test {
    fn cpi(&self) -> f64 {
        if self.rtl.retired == 0 {
            0.0
        } else {
            self.rtl.cycles as f64 / self.rtl.retired as f64
        }
    }
    /// The name without its suite's prefix, which the tables group by.
    fn short(&self) -> &str {
        let n = self.name.as_str();
        n.strip_prefix(&self.suite)
            .map(|s| s.trim_start_matches(['_', '-']))
            .filter(|s| !s.is_empty())
            .unwrap_or(n)
    }
}

fn die(msg: String) -> ! {
    eprintln!("report: {msg}");
    exit(2);
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_else(|e| die(format!("cannot read {}: {e}", p.display())))
}

/// Reads a result file: a first line of `key=value` pairs, then the
/// console output.
fn parse_run(text: &str) -> Run {
    let mut lines = text.splitn(2, '\n');
    let first = lines.next().unwrap_or("");
    let mut run = Run {
        console: lines.next().unwrap_or("").trim().to_string(),
        ..Default::default()
    };
    for kv in first.split_whitespace() {
        if let Some((k, v)) = kv.split_once('=') {
            match k {
                "status" => run.status = v.to_string(),
                "cycles" => run.cycles = v.parse().unwrap_or(0),
                "retired" => run.retired = v.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    run
}

/// Counts the values a test checks: the `.word` lines of its expected
/// results, less the words the framework fills an unused signature
/// region with (`0xdeadbeef`), and less its three canaries and its end
/// canary, which mark the layout and are not results.
fn count_checks(results: &str) -> usize {
    let words: Vec<&str> = results
        .lines()
        .filter_map(|l| l.strip_prefix(".word 0x"))
        .collect();
    let canaries = ["d3a91f6c", "4b8e2d17", "7a110ff5"];
    let n = words
        .iter()
        .filter(|w| **w != "deadbeef" && !canaries.contains(w))
        .count();
    // The last word is the end canary.
    n.saturating_sub(usize::from(words.last().is_some_and(|w| *w != "deadbeef")))
}

/// Reads a workspace status file into a map.
fn status(p: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The first line a tool prints for `--version`.
fn version(tool: &str) -> String {
    Command::new(tool)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

/// The date of a Unix time, as YYYY-MM-DD, in UTC.
fn date(secs: i64) -> String {
    // Howard Hinnant's civil_from_days.
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

/// Escapes text for LaTeX outside verbatim.
fn tex(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '_' | '&' | '%' | '$' | '#' | '{' | '}' => {
                o.push('\\');
                o.push(c);
            }
            '\\' => o.push_str("\\textbackslash{}"),
            '~' => o.push_str("\\textasciitilde{}"),
            '^' => o.push_str("\\textasciicircum{}"),
            _ => o.push(c),
        }
    }
    o
}

/// Escapes text for HTML.
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Formats an integer with thin spaces between groups of three, as
/// the article sets numbers.
fn group(n: u64, sep: &str) -> String {
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            o.push_str(sep);
        }
        o.push(c);
    }
    o
}

/// The status as the article and the page say it: a failure that has
/// an issue is a known failure.
fn verdict(t: &Test) -> &'static str {
    match (t.rtl.status.as_str(), &t.issue) {
        ("PASS", _) => "pass",
        (_, Some(_)) => "known failure",
        ("TIMEOUT", None) => "timeout",
        _ => "fail",
    }
}

/// Per-suite totals.
#[derive(Default)]
struct Suite {
    tests: usize,
    rtl_pass: usize,
    sail_pass: usize,
    checks: usize,
    cycles: u64,
    retired: u64,
}

struct Args {
    manifest: PathBuf,
    stable: PathBuf,
    volatile: PathBuf,
    gcc: String,
    sail: String,
    act_commit: String,
    catalog: usize,
    extensions: Vec<String>,
    excluded: Vec<(String, String)>,
    out: PathBuf,
}

fn args() -> Args {
    let mut a = Args {
        manifest: PathBuf::new(),
        stable: PathBuf::new(),
        volatile: PathBuf::new(),
        gcc: String::new(),
        sail: String::new(),
        act_commit: String::new(),
        catalog: 0,
        extensions: vec![],
        excluded: vec![],
        out: PathBuf::new(),
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < v.len() {
        let val = v
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| die(format!("{} needs a value", v[i])));
        match v[i].as_str() {
            "--manifest" => a.manifest = val.into(),
            "--stable" => a.stable = val.into(),
            "--volatile" => a.volatile = val.into(),
            "--gcc" => a.gcc = val,
            "--sail" => a.sail = val,
            "--act-commit" => a.act_commit = val,
            "--catalog" => a.catalog = val.parse().unwrap_or(0),
            "--extensions" => a.extensions = val.split(',').map(str::to_string).collect(),
            "--excluded" => {
                let (s, why) = val.split_once('=').unwrap_or((&val, ""));
                a.excluded.push((s.to_string(), why.to_string()));
            }
            "--out-dir" => a.out = val.into(),
            o => die(format!("unknown argument {o}")),
        }
        i += 2;
    }
    a
}

fn main() {
    let a = args();
    let mut tests = vec![];
    for line in read(&a.manifest).lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 6 {
            die(format!("bad manifest line: {line}"));
        }
        let checks = if f[4] == "-" {
            0
        } else {
            count_checks(&read(Path::new(f[4])))
        };
        tests.push(Test {
            suite: f[0].to_string(),
            name: f[1].to_string(),
            rtl: parse_run(&read(Path::new(f[2]))),
            sail: parse_run(&read(Path::new(f[3]))),
            checks,
            issue: (f[5] != "-").then(|| f[5].to_string()),
        });
    }
    tests.sort_by(|x, y| (&x.suite, &x.name).cmp(&(&y.suite, &y.name)));

    let stable = status(&a.stable);
    let volatile = status(&a.volatile);
    let get = |m: &BTreeMap<String, String>, k: &str| {
        m.get(k).cloned().unwrap_or_else(|| "unstamped".into())
    };
    let repo_commit = get(&stable, "STABLE_GIT_COMMIT");
    let txhdl_commit = get(&stable, "STABLE_TXHDL_COMMIT");
    let day = volatile
        .get("BUILD_TIMESTAMP")
        .and_then(|t| t.parse::<i64>().ok())
        .map(date)
        .unwrap_or_else(|| "unstamped".into());
    let gcc = version(&a.gcc);
    let sail = format!("Sail RISC-V model {}", version(&a.sail));
    let short = |s: &str| s.chars().take(12).collect::<String>();

    let mut suites: BTreeMap<String, Suite> = BTreeMap::new();
    for t in &tests {
        let s = suites.entry(t.suite.clone()).or_default();
        s.tests += 1;
        s.rtl_pass += usize::from(t.rtl.status == "PASS");
        s.sail_pass += usize::from(t.sail.status == "PASS");
        s.checks += t.checks;
        s.cycles += t.rtl.cycles;
        s.retired += t.rtl.retired;
    }
    let n = tests.len();
    let rtl_pass = tests.iter().filter(|t| t.rtl.status == "PASS").count();
    let sail_pass = tests.iter().filter(|t| t.sail.status == "PASS").count();
    let known = tests
        .iter()
        .filter(|t| t.rtl.status != "PASS" && t.issue.is_some())
        .count();
    let checks: usize = tests.iter().map(|t| t.checks).sum();
    let cycles: u64 = tests.iter().map(|t| t.rtl.cycles).sum();
    let retired: u64 = tests.iter().map(|t| t.rtl.retired).sum();
    let cpi = if retired == 0 {
        0.0
    } else {
        cycles as f64 / retired as f64
    };
    let failing: Vec<&Test> = tests.iter().filter(|t| t.rtl.status != "PASS").collect();

    fs::create_dir_all(&a.out).unwrap_or_else(|e| die(format!("{e}")));
    let w = |name: &str, body: String| {
        fs::write(a.out.join(name), body)
            .unwrap_or_else(|e| die(format!("cannot write {name}: {e}")))
    };

    // results.tex: the numbers, as macros.
    let mut m = String::new();
    let mut def = |k: &str, v: String| writeln!(m, "\\newcommand{{\\{k}}}{{{v}}}").unwrap();
    def("NTests", n.to_string());
    def("NSuites", suites.len().to_string());
    def("NCatalog", a.catalog.to_string());
    def("NRtlPass", rtl_pass.to_string());
    def("NRtlFail", (n - rtl_pass).to_string());
    def("NSailPass", sail_pass.to_string());
    def("NKnownFail", known.to_string());
    def("NUnknownFail", (n - rtl_pass - known).to_string());
    def("NChecks", group(checks as u64, "\\,"));
    def("TotalCycles", group(cycles, "\\,"));
    def("TotalInstr", group(retired, "\\,"));
    def("MeanCPI", format!("{cpi:.2}"));
    def(
        "PassPercent",
        format!("{:.1}", 100.0 * rtl_pass as f64 / n.max(1) as f64),
    );
    def("RepoCommit", tex(&short(&repo_commit)));
    def("TxhdlCommit", tex(&short(&txhdl_commit)));
    def("ActCommit", tex(&short(&a.act_commit)));
    def("ReportDate", day.clone());
    def("SailVersion", tex(&sail));
    def("GccVersion", tex(&gcc));
    def("Extensions", tex(&a.extensions.join(", ")));
    // The suites with the most and the fewest cycles per instruction,
    // and with the most and the fewest checks, for the prose.
    let suite_cpi = |v: &Suite| {
        if v.retired == 0 {
            0.0
        } else {
            v.cycles as f64 / v.retired as f64
        }
    };
    let by_cpi = |hi: bool| {
        suites
            .iter()
            .max_by(|x, y| {
                let o = suite_cpi(x.1).total_cmp(&suite_cpi(y.1));
                if hi {
                    o
                } else {
                    o.reverse()
                }
            })
            .map(|(k, v)| (k.clone(), suite_cpi(v)))
            .unwrap_or_default()
    };
    let (slow, slow_cpi) = by_cpi(true);
    let (fast, fast_cpi) = by_cpi(false);
    def("SlowSuite", tex(&slow));
    def("SlowCPI", format!("{slow_cpi:.2}"));
    def("FastSuite", tex(&fast));
    def("FastCPI", format!("{fast_cpi:.2}"));
    let most = suites
        .iter()
        .max_by_key(|x| x.1.checks)
        .map(|(k, v)| (k.clone(), v.checks, v.tests))
        .unwrap_or_default();
    def("MostChecksSuite", tex(&most.0));
    def("MostChecks", group(most.1 as u64, "\\,"));
    def("MostChecksTests", most.2.to_string());
    w("results.tex", m);

    // suites.tex: the table of suites, a row each. The whole tabular is
    // written here: LaTeX cannot `\\input` rows and then a rule.
    let mut s = String::from(
        "\\begin{tabular}{@{}l r r r r r@{}}\n\\toprule\n\
         Suite & Tests & Sail & Netlist & Checks & CPI \\\\\n\\midrule\n",
    );
    for (name, v) in &suites {
        writeln!(
            s,
            "\\code{{{}}} & {} & {} & {} & {} & {:.2} \\\\",
            tex(name),
            v.tests,
            v.sail_pass,
            v.rtl_pass,
            group(v.checks as u64, "\\,"),
            if v.retired == 0 {
                0.0
            } else {
                v.cycles as f64 / v.retired as f64
            },
        )
        .unwrap();
    }
    writeln!(s, "\\midrule").unwrap();
    writeln!(
        s,
        "Total & {n} & {sail_pass} & {rtl_pass} & {} & {cpi:.2} \\\\",
        group(checks as u64, "\\,")
    )
    .unwrap();
    s.push_str("\\bottomrule\n\\end{tabular}\n");
    w("suites.tex", s);

    // tests.tex: the per-test tables, a float per chunk of rows.
    let rows_per_table = 44;
    let chunks: Vec<&[Test]> = tests.chunks(rows_per_table).collect();
    let mut t = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        writeln!(t, "\\begin{{table}}[!p]").unwrap();
        writeln!(t, "\\centering").unwrap();
        writeln!(
            t,
            "\\caption{{Every test on the netlist ({} of {})}}",
            i + 1,
            chunks.len()
        )
        .unwrap();
        if i == 0 {
            writeln!(t, "\\label{{tab:tests}}").unwrap();
        }
        writeln!(t, "\\tiny").unwrap();
        writeln!(t, "\\begin{{tabular}}{{@{{}}l r r r l@{{}}}}").unwrap();
        writeln!(t, "\\toprule").unwrap();
        writeln!(t, "Test & Checks & Cycles & CPI & Result \\\\").unwrap();
        writeln!(t, "\\midrule").unwrap();
        let mut last = "";
        for x in chunk.iter() {
            if x.suite != last {
                writeln!(
                    t,
                    "\\multicolumn{{5}}{{@{{}}l}}{{\\textbf{{{}}}}} \\\\",
                    tex(&x.suite)
                )
                .unwrap();
                last = &x.suite;
            }
            let v = verdict(x);
            let v = if v == "pass" {
                v.to_string()
            } else {
                format!("\\textbf{{{v}}}")
            };
            writeln!(
                t,
                "\\quad\\texttt{{{}}} & {} & {} & {:.2} & {} \\\\",
                tex(x.short()),
                x.checks,
                group(x.rtl.cycles, "\\,"),
                x.cpi(),
                v
            )
            .unwrap();
        }
        writeln!(t, "\\bottomrule").unwrap();
        writeln!(t, "\\end{{tabular}}").unwrap();
        writeln!(t, "\\end{{table}}").unwrap();
    }
    w("tests.tex", t);

    // failures.tex: what each failing test said.
    let mut f = String::new();
    if failing.is_empty() {
        writeln!(f, "No test failed on the netlist in this run.").unwrap();
    }
    for x in &failing {
        writeln!(f, "\\subsection{{\\texttt{{{}}}}}", tex(&x.name)).unwrap();
        write!(
            f,
            "The run ended with status {} after {} cycles and {} instructions.",
            x.rtl.status,
            group(x.rtl.cycles, "\\,"),
            group(x.rtl.retired, "\\,")
        )
        .unwrap();
        if let Some(url) = &x.issue {
            write!(f, " The failure is tracked as \\url{{{url}}}.").unwrap();
        }
        writeln!(f, " The test printed the following on the console.").unwrap();
        writeln!(f, "\\begin{{lstlisting}}[style=console]").unwrap();
        for line in x.rtl.console.lines().filter(|l| !l.trim().is_empty()) {
            let line = line.strip_prefix("RVCP: ").unwrap_or(line);
            writeln!(f, "{line}").unwrap();
        }
        writeln!(f, "\\end{{lstlisting}}").unwrap();
    }
    w("failures.tex", f);

    // cpichart.tex: mean cycles per instruction by suite, as bars.
    let mut c = String::new();
    let max = suites
        .values()
        .map(|v| {
            if v.retired == 0 {
                0.0
            } else {
                v.cycles as f64 / v.retired as f64
            }
        })
        .fold(1.0_f64, f64::max);
    let scale = 4.0 / max.ceil();
    writeln!(
        c,
        "\\begin{{tikzpicture}}[x=1cm,y=0.32cm,font=\\scriptsize]"
    )
    .unwrap();
    let k = suites.len();
    for (i, (name, v)) in suites.iter().enumerate() {
        let y = (k - 1 - i) as f64;
        let cpi = if v.retired == 0 {
            0.0
        } else {
            v.cycles as f64 / v.retired as f64
        };
        writeln!(
            c,
            "\\node[anchor=east,font=\\tiny\\ttfamily] at (0,{y}) {{{}}};",
            tex(name)
        )
        .unwrap();
        writeln!(
            c,
            "\\fill[barfill] (0.05,{:.2}) rectangle ({:.3},{:.2});",
            y - 0.35,
            0.05 + cpi * scale,
            y + 0.35
        )
        .unwrap();
        writeln!(
            c,
            "\\node[anchor=west,font=\\tiny] at ({:.3},{y}) {{{cpi:.2}}};",
            0.1 + cpi * scale
        )
        .unwrap();
    }
    let top = max.ceil() as u64;
    writeln!(
        c,
        "\\draw (0.05,-0.8) -- ({:.3},-0.8);",
        0.05 + top as f64 * scale
    )
    .unwrap();
    for g in 0..=top {
        let x = 0.05 + g as f64 * scale;
        writeln!(
            c,
            "\\draw ({x:.3},-0.8) -- ({x:.3},-1.0) node[below,font=\\tiny] {{{g}}};"
        )
        .unwrap();
    }
    writeln!(
        c,
        "\\node[font=\\tiny] at ({:.3},-2.4) {{cycles per instruction}};",
        0.05 + top as f64 * scale / 2.0
    )
    .unwrap();
    writeln!(c, "\\end{{tikzpicture}}").unwrap();
    w("cpichart.tex", c);

    // results.tsv.
    let mut tsv = String::from("suite\ttest\tsail\trtl\tchecks\tcycles\tinstructions\tissue\n");
    for x in &tests {
        writeln!(
            tsv,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            x.suite,
            x.name,
            x.sail.status,
            x.rtl.status,
            x.checks,
            x.rtl.cycles,
            x.rtl.retired,
            x.issue.as_deref().unwrap_or("")
        )
        .unwrap();
    }
    w("results.tsv", tsv);

    w(
        "report.html",
        page(&Page {
            tests: &tests,
            suites: &suites,
            failing: &failing,
            n,
            rtl_pass,
            sail_pass,
            known,
            checks,
            cpi,
            catalog: a.catalog,
            day: &day,
            repo_commit: &repo_commit,
            txhdl_commit: &txhdl_commit,
            act_commit: &a.act_commit,
            gcc: &gcc,
            sail: &sail,
            extensions: &a.extensions,
            excluded: &a.excluded,
        }),
    );
}

struct Page<'a> {
    tests: &'a [Test],
    suites: &'a BTreeMap<String, Suite>,
    failing: &'a [&'a Test],
    n: usize,
    rtl_pass: usize,
    sail_pass: usize,
    known: usize,
    checks: usize,
    cpi: f64,
    catalog: usize,
    day: &'a str,
    repo_commit: &'a str,
    txhdl_commit: &'a str,
    act_commit: &'a str,
    gcc: &'a str,
    sail: &'a str,
    extensions: &'a [String],
    excluded: &'a [(String, String)],
}

/// The HTML page: one file, no scripts from elsewhere, light and dark.
fn page(p: &Page) -> String {
    let link = |base: &str, sha: &str| {
        if sha == "unstamped" {
            html(sha)
        } else {
            format!(
                "<a href=\"{base}{sha}\"><code>{}</code></a>",
                html(&sha.chars().take(12).collect::<String>())
            )
        }
    };
    let mut h = String::new();
    h.push_str(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Vreteno RISC-V conformance</title>
<style>
:root { --bg:#fbfbf9; --fg:#1d1d1b; --muted:#5f5f5a; --rule:#d9d8d2; --pass:#1f6f3f; --fail:#a12622; --known:#8a5a00; --card:#ffffff; --bar:#3b5b92; }
@media (prefers-color-scheme: dark) { :root { --bg:#151614; --fg:#e8e7e2; --muted:#a4a39c; --rule:#3a3a36; --pass:#6fcf8f; --fail:#ff8a80; --known:#f0b44c; --card:#1e1f1c; --bar:#8fb0e8; } }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--fg); font:16px/1.5 "Latin Modern Roman","Computer Modern Serif",Georgia,serif; }
main { max-width:960px; margin:0 auto; padding:24px 16px 64px; }
h1 { font-size:1.9rem; margin:0 0 4px; } h2 { margin-top:2.2rem; border-bottom:1px solid var(--rule); padding-bottom:4px; }
.sub { color:var(--muted); margin:0 0 20px; }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(170px,1fr)); gap:12px; }
.card { background:var(--card); border:1px solid var(--rule); border-radius:6px; padding:12px 14px; }
.card b { display:block; font-size:1.7rem; font-variant-numeric:tabular-nums; }
.card span { color:var(--muted); font-size:.9rem; }
.wrap { overflow-x:auto; }
table { border-collapse:collapse; width:100%; font-size:.92rem; font-variant-numeric:tabular-nums; }
th, td { text-align:left; padding:4px 8px; border-bottom:1px solid var(--rule); white-space:nowrap; }
td.n, th.n { text-align:right; }
.pass { color:var(--pass); } .fail { color:var(--fail); font-weight:bold; } .known { color:var(--known); font-weight:bold; }
code, pre { font-family:"Latin Modern Mono","Computer Modern Typewriter",ui-monospace,monospace; font-size:.9em; }
pre { background:var(--card); border:1px solid var(--rule); padding:10px; overflow-x:auto; }
a { color:var(--bar); }
.bar { display:inline-block; height:.7em; background:var(--bar); vertical-align:middle; margin-right:6px; }
dl { display:grid; grid-template-columns:max-content 1fr; gap:4px 16px; } dt { color:var(--muted); } dd { margin:0; }
</style>
</head>
<body>
<main>
<h1>Vreteno RISC-V conformance</h1>
"#,
    );
    writeln!(
        h,
        "<p class=\"sub\">The RISC-V architectural tests on the Vreteno RV32IMAC core, run on {}. \
         Read the <a href=\"vreteno-conformance.pdf\">full report (PDF)</a>.</p>",
        html(p.day)
    )
    .unwrap();
    writeln!(h, "<div class=\"cards\">").unwrap();
    let card = |h: &mut String, v: String, l: &str| {
        writeln!(h, "<div class=\"card\"><b>{v}</b><span>{l}</span></div>").unwrap()
    };
    card(
        &mut h,
        format!("{}&#8202;/&#8202;{}", p.rtl_pass, p.n),
        "tests pass on the netlist",
    );
    card(
        &mut h,
        format!("{}&#8202;/&#8202;{}", p.sail_pass, p.n),
        "pass their self-check on Sail",
    );
    card(
        &mut h,
        p.known.to_string(),
        "known failures, each with an issue",
    );
    card(
        &mut h,
        group(p.checks as u64, ","),
        "results checked against Sail",
    );
    card(&mut h, format!("{:.2}", p.cpi), "cycles per instruction");
    writeln!(h, "</div>").unwrap();

    writeln!(h, "<h2>Failures</h2>").unwrap();
    if p.failing.is_empty() {
        writeln!(h, "<p>No test failed on the netlist in this run.</p>").unwrap();
    }
    for x in p.failing {
        let cls = if x.issue.is_some() { "known" } else { "fail" };
        write!(
            h,
            "<h3><code>{}</code> <span class=\"{cls}\">{}</span></h3>",
            html(&x.name),
            verdict(x)
        )
        .unwrap();
        if let Some(u) = &x.issue {
            writeln!(h, "<p>Tracked as <a href=\"{0}\">{0}</a>.</p>", html(u)).unwrap();
        }
        writeln!(h, "<pre>{}</pre>", html(x.rtl.console.trim())).unwrap();
    }

    writeln!(h, "<h2>By suite</h2><div class=\"wrap\"><table>").unwrap();
    writeln!(
        h,
        "<tr><th>Suite</th><th class=\"n\">Tests</th><th class=\"n\">Sail</th><th class=\"n\">Netlist</th>\
         <th class=\"n\">Checks</th><th>Cycles per instruction</th></tr>"
    )
    .unwrap();
    let max = p
        .suites
        .values()
        .map(|v| {
            if v.retired == 0 {
                0.0
            } else {
                v.cycles as f64 / v.retired as f64
            }
        })
        .fold(1.0_f64, f64::max);
    for (name, v) in p.suites {
        let cpi = if v.retired == 0 {
            0.0
        } else {
            v.cycles as f64 / v.retired as f64
        };
        let cls = if v.rtl_pass == v.tests {
            "pass"
        } else {
            "fail"
        };
        writeln!(
            h,
            "<tr><td><code>{}</code></td><td class=\"n\">{}</td><td class=\"n\">{}</td>\
             <td class=\"n {cls}\">{}</td><td class=\"n\">{}</td>\
             <td><span class=\"bar\" style=\"width:{:.0}px\"></span>{cpi:.2}</td></tr>",
            html(name),
            v.tests,
            v.sail_pass,
            v.rtl_pass,
            group(v.checks as u64, ","),
            120.0 * cpi / max
        )
        .unwrap();
    }
    writeln!(h, "</table></div>").unwrap();

    writeln!(h, "<h2>Every test</h2><div class=\"wrap\"><table>").unwrap();
    writeln!(
        h,
        "<tr><th>Test</th><th>Sail</th><th>Netlist</th><th class=\"n\">Checks</th>\
         <th class=\"n\">Cycles</th><th class=\"n\">Instructions</th></tr>"
    )
    .unwrap();
    for x in p.tests {
        let v = verdict(x);
        let cls = match v {
            "pass" => "pass",
            "known failure" => "known",
            _ => "fail",
        };
        let sail_cls = if x.sail.status == "PASS" {
            "pass"
        } else {
            "fail"
        };
        writeln!(
            h,
            "<tr><td><code>{}</code></td><td class=\"{sail_cls}\">{}</td><td class=\"{cls}\">{v}</td>\
             <td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
            html(&x.name),
            html(&x.sail.status.to_lowercase()),
            x.checks,
            group(x.rtl.cycles, ","),
            group(x.rtl.retired, ",")
        )
        .unwrap();
    }
    writeln!(h, "</table></div>").unwrap();

    writeln!(h, "<h2>How this was run</h2><dl>").unwrap();
    writeln!(
        h,
        "<dt>Core</dt><dd>Vreteno, TxHDL {}</dd>",
        link(
            "https://github.com/filmil/hdl-txhdl/commit/",
            p.txhdl_commit
        )
    )
    .unwrap();
    writeln!(
        h,
        "<dt>Tests</dt><dd>riscv-arch-test (ACT4) {}; {} of {} tests apply</dd>",
        link(
            "https://github.com/riscv-non-isa/riscv-arch-test/commit/",
            p.act_commit
        ),
        p.n,
        p.catalog
    )
    .unwrap();
    writeln!(h, "<dt>Reference</dt><dd>{}</dd>", html(p.sail)).unwrap();
    writeln!(h, "<dt>Compiler</dt><dd>{}</dd>", html(p.gcc)).unwrap();
    writeln!(
        h,
        "<dt>Simulator</dt><dd>Verilator, on the netlist TxHDL writes</dd>"
    )
    .unwrap();
    writeln!(
        h,
        "<dt>Extensions</dt><dd>{}</dd>",
        html(&p.extensions.join(", "))
    )
    .unwrap();
    for (s, why) in p.excluded {
        writeln!(
            h,
            "<dt>Left out</dt><dd><code>{}</code>: {}</dd>",
            html(s),
            html(why)
        )
        .unwrap();
    }
    writeln!(
        h,
        "<dt>Harness</dt><dd>{}</dd>",
        link(
            "https://github.com/filmil/vreteno-conformance/commit/",
            p.repo_commit
        )
    )
    .unwrap();
    writeln!(h, "</dl>\n</main>\n</body>\n</html>").unwrap();
    h
}
