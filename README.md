<!-- SPDX-License-Identifier: Apache-2.0 -->
[![test](https://github.com/filmil/vreteno-conformance/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/filmil/vreteno-conformance/actions/workflows/test.yml)
[![daily](https://github.com/filmil/vreteno-conformance/actions/workflows/daily.yml/badge.svg?branch=main)](https://github.com/filmil/vreteno-conformance/actions/workflows/daily.yml)

# Vreteno RISC-V conformance

This repository runs the official RISC-V architectural tests on
[Vreteno](https://github.com/filmil/hdl-txhdl), the RV32IMAC core of the
TxHDL project.
It runs them every day, against the newest commit of the core.

* The tests are RISC-V International's
  [architectural certification tests](https://github.com/riscv-non-isa/riscv-arch-test)
  (ACT4).
* The [Sail RISC-V model](https://github.com/riscv/sail-riscv) computes
  the result each test must reproduce.
* Each test runs on the core's Verilog netlist under
  [Verilator](https://verilator.org).

The result is a report, an IEEE two-column article.
The latest one is on
[hdlfactory.com](https://www.hdlfactory.com/vreteno-conformance/), as a
web page and as a PDF.
Every daily report is also a
[release](https://github.com/filmil/vreteno-conformance/releases) of this
repository.

## Building

Everything is built by Bazel, and Bazel fetches every tool by checksum
or by commit: the core and its C, C++ and Rust toolchains from TxHDL,
GCC 15 for RISC-V, Sail, the tests, Verilator and a TeX distribution.
The machine needs `bazelisk` and Git, and nothing else.

```sh
bazel test //...                                 # every test, both simulators
bazel test //tests:vreteno_rtl_tests             # the netlist only
bazel build //paper:vreteno-conformance          # the report, as a PDF
bazel build //tests:report                       # its data, and report.html
```

`bazel test` passes when every test passes on the netlist, except the
known failures.
A known failure is a test the core is known to fail, listed in
`tests/BUILD.bazel` with the URL of the issue that tracks it.
Its check passes while the test fails, and fails once the test passes,
so the entry is removed when the core is fixed.
The report counts known failures as failures.

To run against a TxHDL checkout instead of the pinned commit:

```sh
TXHDL_COMMIT=$(git -C ../hdl-txhdl rev-parse HEAD) \
  bazel test --override_module=txhdl=$PWD/../hdl-txhdl //...
```

## Layout

* `dut/` is the device under test: a TxHDL unit that joins the Vreteno
  hart, its AXI tracker and AXI4 pins, lowered to Verilog.
* `sim/` is the Verilator testbench: an AXI4 memory, the ELF loader, and
  the HTIF `tohost` console and exit.
* `act/` holds the Bazel rules that build and run the tests, and
  `act/config/` describes Vreteno to the tests and to Sail.
* `tests/` selects the tests that apply to Vreteno, lists the known
  failures, and makes the report's data.
* `paper/` is the report.
* `tools/` holds `sigfmt`, `report` and `stamp`, small Rust programs the
  rules run.

## Workflows

* `.github/workflows/test.yml` runs `bazel build //...` and
  `bazel test //...` on every pull request, every push to `main`, and
  once a week.
* `.github/workflows/daily.yml` runs every day at 04:00 UTC, and on
  `workflow_dispatch`.
  It builds against the newest TxHDL commit on GitHub, publishes the
  report as a release named `report-YYYY-MM-DD`, and copies the web page
  and the PDF to `static/vreteno-conformance` in
  `filmil/hdlfactory.com.template`.
  It skips the release and the copy when neither this repository nor
  TxHDL has changed since the last release.
