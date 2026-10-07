<!-- SPDX-License-Identifier: Apache-2.0 -->
# Instructions

Read `README.md` first.
It says what this repository is and how to build it.

The coding standard is the `ai-coding-sop` repository at
`https://github.com/filmil/ai-coding-sop`.
Its `AGENTS.md`, its `prose-readability` skill and its
`git-commit-rules` skill apply here in full.
In particular: no em-dashes or en-dashes anywhere, one sentence per line
in Markdown, conventional-commit titles, and the assistant note plus the
exact prompt appended to every commit message.

# Standing rules

## The build is hermetic

Every tool comes from `MODULE.bazel`.
Nothing is installed on the machine beyond `bazelisk` and Git, and the
workflows install nothing else.
A rule or a test that calls a tool from `PATH` is a defect; `act/check.sh`
uses shell builtins only for that reason.

## The description of Vreteno is three files that agree

`act/config/rvtest_config.h`, `act/config/sail.json` and the lists
`VRETENO_EXTENSIONS` and `VRETENO_PARAMS` in `tests/BUILD.bazel` describe
the same core.
A change to one is a change to all three, in the same commit.
Each claim in them is checked against the core's source in TxHDL,
`cpu/vreteno/src/isa.rs` and `cpu/vreteno/src/core.rs`, and the report's
Table I states the choices that matter.
The Sail self-check, `bazel test //tests:vreteno_sail_tests`, catches a
description that disagrees with itself, not one that disagrees with the
core.

## A failure is a bug in the core, a bug in the description, or neither

When a test fails on the netlist, read its console in
`bazel-bin/tests/<test>_rtl_run.result`.
If the specification allows the core's behaviour, the description is
wrong: fix `sail.json` and `rvtest_config.h`.
If it does not, the core is wrong: file an issue on `HDL/txhdl` at
`git.hdlfactory.com`, as TxHDL's own `AGENTS.md` requires, and add the
test to `VRETENO_KNOWN_FAILURES` in `tests/BUILD.bazel` with the issue's
URL.
Never add a known failure without an issue.

## The report states only what the build measured

Every number in `paper/` comes from `//tests:report`, which
`tools/report` writes from the result files.
A number typed into a section is a defect, because nothing updates it
the next day.
Prose that describes one run names the run's date and commit.
Before committing a change to the report, build it, render every page
with `pdftoppm`, and look at each figure and table.
