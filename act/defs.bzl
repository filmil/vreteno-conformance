# SPDX-License-Identifier: Apache-2.0
"""Rules that build and run the RISC-V architectural tests.

The ACT4 framework turns one test source into one self-checking ELF in
four steps, in `framework/src/act/build_plan.py`. `act_elf` repeats them
as Bazel actions:

1. compile the test with `-DSIGNATURE`, which makes it write its
   results into a signature region;
2. run that ELF on the Sail reference model, which writes the
   signature to a file;
3. format the signature as assembler source, with `//tools/sigfmt`;
4. compile the test again with `-DRVTEST_SELFCHECK`, including that
   source, so the test compares every result it computes with the
   one Sail computed, and says PASS or FAIL itself.

`act_run` runs the final ELF on a simulator and writes a result file.
A run does not fail the build when the test fails: the result file
records the failure, the test target reads it, and the report counts
it.

`act_suite` makes these targets for every test in the catalog that
applies to a device, and selects tests the way the ACT4 framework does
in `framework/src/act/select_tests.py`.
"""

load("@rules_shell//shell:sh_test.bzl", "sh_test")

ActElfInfo = provider(
    doc = "What act_elf built for one test.",
    fields = {
        "elf": "File: the self-checking ELF.",
        "results": "File or None: the expected results, as assembler " +
                   "source, one `.word` per value the test checks.",
    },
)

# The XLEN every rule here builds for.
_XLEN = 32

def _config_dir(ctx):
    return ctx.file.rvmodel_macros.dirname

def _env_dir(ctx):
    for f in ctx.files.env:
        if f.basename == "riscv_arch_test.h":
            return f.dirname
    fail("the test environment has no riscv_arch_test.h")

def _march_flags(march):
    # As `Toolchain.march_flags` does for GCC and an assembly test: the
    # driver gets the base ISA, and the assembler gets the full string.
    march = march.replace("${XLEN}", str(_XLEN))
    return ["-march=rv%di" % _XLEN, "-Xassembler", "-march=" + march]

def _compile(ctx, out, extra, extra_inputs, mnemonic):
    test = ctx.file.src
    args = ctx.actions.args()
    args.add("-Wl,--no-warn-rwx-segments")
    args.add("-I" + _config_dir(ctx))
    args.add("-T" + ctx.file.linker_script.path)
    args.add_all(["-O0", "-g", "-mcmodel=medany", "-nostdlib"])
    args.add("-I" + _env_dir(ctx))
    args.add("-I.")
    args.add("-o", out)
    args.add_all(_march_flags(ctx.attr.march))
    args.add("-mabi=ilp32")
    args.add_all(extra)
    args.add("-DTEST_FLEN=32")
    args.add("-DTEST_FILE=\"%s\"" % test.basename)
    args.add(test)
    ctx.actions.run(
        executable = ctx.file._gcc,
        arguments = [args],
        inputs = depset(
            [test, ctx.file.linker_script, ctx.file.rvmodel_macros, ctx.file.rvtest_config] +
            ctx.files.env + extra_inputs,
            transitive = [depset(ctx.files._gcc_all)],
        ),
        outputs = [out],
        mnemonic = mnemonic,
        progress_message = "%s %s" % (mnemonic, ctx.label.name),
    )

def _act_elf_impl(ctx):
    name = ctx.label.name
    final_elf = ctx.actions.declare_file(name + ".elf")
    outputs = [final_elf]
    results = None
    if ctx.attr.needs_signature:
        sig_elf = ctx.actions.declare_file(name + ".sig.elf")
        sig = ctx.actions.declare_file(name + ".sig")
        sig_log = ctx.actions.declare_file(name + ".sig.log")
        results = ctx.actions.declare_file(name + ".results")
        _compile(
            ctx,
            sig_elf,
            [
                "-DSIGNATURE",
                # Sail's devices, which `sail_macros.h` reaches.
                "-DSAIL_CLINT_BASE_ADDRESS=0x%x" % ctx.attr.sail_clint_base,
                "-DSAIL_SIMPLE_INTERRUPT_GENERATOR_BASE_ADDRESS=0x%x" % ctx.attr.sail_sig_base,
            ],
            [],
            "ActSigCompile",
        )
        ctx.actions.run_shell(
            command = """
"$1" --config "$2" --test-signature="$3" --signature-granularity 4 "$4" > "$5" 2>&1 || {
  echo "Sail failed on $4; its log follows." >&2
  while IFS= read -r line; do echo "$line" >&2; done < "$5"
  exit 1
}
""",
            arguments = [
                ctx.file._sail.path,
                ctx.file.sail_config.path,
                sig.path,
                sig_elf.path,
                sig_log.path,
            ],
            inputs = [sig_elf, ctx.file.sail_config],
            tools = [ctx.file._sail],
            outputs = [sig, sig_log],
            mnemonic = "ActSail",
            progress_message = "Running Sail for the signature of %s" % name,
        )
        ctx.actions.run(
            executable = ctx.executable._sigfmt,
            arguments = [sig.path, results.path],
            inputs = [sig],
            outputs = [results],
            mnemonic = "ActSigFmt",
        )
        _compile(
            ctx,
            final_elf,
            [
                "-DRVTEST_SELFCHECK",
                "-DSIGNATURE_FILE=\"%s\"" % results.path,
                "-DXLEN=%d" % _XLEN,
            ],
            [results],
            "ActCompile",
        )
        outputs += [sig, results]
    else:
        _compile(
            ctx,
            final_elf,
            ["-DRVTEST_SELFCHECK", "-DRVTEST_NOSIG", "-DXLEN=%d" % _XLEN],
            [],
            "ActCompile",
        )
    objdump = ctx.actions.declare_file(name + ".elf.objdump")
    ctx.actions.run_shell(
        command = "\"$1\" -d -M no-aliases,numeric \"$2\" > \"$3\"",
        arguments = [ctx.file._objdump.path, final_elf.path, objdump.path],
        inputs = [final_elf],
        tools = [ctx.file._objdump],
        outputs = [objdump],
        mnemonic = "ActObjdump",
    )
    return [
        ActElfInfo(
            elf = final_elf,
            results = results if ctx.attr.needs_signature else None,
        ),
        DefaultInfo(files = depset([final_elf])),
        OutputGroupInfo(
            debug = depset(outputs + [objdump]),
        ),
    ]

act_elf = rule(
    implementation = _act_elf_impl,
    doc = "Builds one architectural test as a self-checking ELF.",
    attrs = {
        "src": attr.label(
            allow_single_file = [".S"],
            mandatory = True,
            doc = "The test source.",
        ),
        "march": attr.string(
            mandatory = True,
            doc = "The test's MARCH, from its header.",
        ),
        "needs_signature": attr.bool(
            default = True,
            doc = "Whether the test checks results computed by Sail.",
        ),
        "env": attr.label(
            default = "@riscv_arch_test//:env",
            doc = "The shared test environment headers.",
        ),
        "rvmodel_macros": attr.label(
            allow_single_file = True,
            default = "//act/config:rvmodel_macros.h",
            doc = "The device's macros. Its directory is on the include path.",
        ),
        "rvtest_config": attr.label(
            allow_single_file = True,
            default = "//act/config:rvtest_config.h",
            doc = "The device's configuration, beside rvmodel_macros.h.",
        ),
        "linker_script": attr.label(
            allow_single_file = True,
            default = "//act/config:link.ld",
            doc = "The device's memory map.",
        ),
        "sail_config": attr.label(
            allow_single_file = True,
            default = "//act/config:sail.json",
            doc = "The Sail configuration that matches the device.",
        ),
        "sail_clint_base": attr.int(
            default = 0x2000000,
            doc = "platform.clint.base in sail_config.",
        ),
        "sail_sig_base": attr.int(
            default = 0xc000000,
            doc = "platform.simple_interrupt_generator.base in sail_config.",
        ),
        "_gcc": attr.label(
            allow_single_file = True,
            default = "@riscv_gcc//:gcc",
        ),
        "_gcc_all": attr.label(
            default = "@riscv_gcc//:all",
        ),
        "_objdump": attr.label(
            allow_single_file = True,
            default = "@riscv_gcc//:objdump",
        ),
        "_sail": attr.label(
            allow_single_file = True,
            default = "@sail_riscv//:sail",
        ),
        "_sigfmt": attr.label(
            executable = True,
            cfg = "exec",
            default = "//tools/sigfmt",
        ),
    },
)

def _act_run_impl(ctx):
    name = ctx.attr.test_name
    result = ctx.actions.declare_file(ctx.label.name + ".result")
    elf = ctx.file.elf
    if ctx.attr.simulator == "rtl":
        ctx.actions.run(
            executable = ctx.executable._sim,
            arguments = [
                "--elf",
                elf.path,
                "--out",
                result.path,
                "--name",
                name,
                "--max-cycles",
                str(ctx.attr.max_cycles),
            ],
            inputs = [elf],
            outputs = [result],
            mnemonic = "ActRunRtl",
            progress_message = "Running %s on the Vreteno netlist" % name,
        )
    else:
        log = ctx.actions.declare_file(ctx.label.name + ".log")
        ctx.actions.run_shell(
            command = """
if "$1" --config "$2" "$3" > "$4" 2>&1; then s=PASS; else s=FAIL; fi
echo "name=$5 status=$s" > "$6"
""",
            arguments = [
                ctx.file._sail.path,
                ctx.file.sail_config.path,
                elf.path,
                log.path,
                name,
                result.path,
            ],
            inputs = [elf, ctx.file.sail_config],
            tools = [ctx.file._sail],
            outputs = [result, log],
            mnemonic = "ActRunSail",
            progress_message = "Running %s on Sail" % name,
        )
    return [DefaultInfo(files = depset([result]))]

act_run = rule(
    implementation = _act_run_impl,
    doc = "Runs one self-checking test ELF and writes its result file.",
    attrs = {
        "elf": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "The self-checking ELF, from act_elf.",
        ),
        "test_name": attr.string(
            mandatory = True,
            doc = "The test's name, written into the result.",
        ),
        "simulator": attr.string(
            values = ["rtl", "sail"],
            mandatory = True,
            doc = "`rtl` for the Vreteno netlist under Verilator, " +
                  "`sail` for the reference model.",
        ),
        "max_cycles": attr.int(
            default = 20000000,
            doc = "The cycles after which an RTL run is a timeout.",
        ),
        "sail_config": attr.label(
            allow_single_file = True,
            default = "//act/config:sail.json",
        ),
        "_sim": attr.label(
            executable = True,
            cfg = "exec",
            default = "//sim",
        ),
        "_sail": attr.label(
            allow_single_file = True,
            default = "@sail_riscv//:sail",
        ),
    },
)

def _compare(test_value, config_value):
    """`_compare_param` from select_tests.py."""
    if type(test_value) == "string":
        for op in [">=", "<=", "!=", "==", ">", "<"]:
            if test_value.startswith(op):
                v = test_value[len(op):].strip()
                req = int(v, 16) if v.lower().startswith("0x") else int(v)
                if type(config_value) != "int":
                    return False
                return {
                    ">=": config_value >= req,
                    "<=": config_value <= req,
                    "!=": config_value != req,
                    "==": config_value == req,
                    ">": config_value > req,
                    "<": config_value < req,
                }[op]
    return test_value == config_value

def select_tests(tests, extensions, params, exclude_suites = []):
    """Selects the tests that apply to a device.

    Args:
      tests: the catalog, `TESTS` from `@riscv_arch_test//:tests.bzl`.
      extensions: the extensions the device implements, as UDB names
        them.
      params: the device's parameters, as a dict from the names the
        tests' headers use.
      exclude_suites: suites, by directory name, left out by hand.

    Returns:
      The selected entries of `tests`.
    """
    out = []
    for t in tests:
        if t["suite"] in exclude_suites or t["min_harts"] > 1:
            continue
        if [e for e in t["forbidden"] if e in extensions]:
            continue
        ok = True
        for r in t["required"]:
            if type(r) == "string":
                ok = ok and r in extensions
            else:
                ok = ok and len([e for e in r if e in extensions]) > 0
        for k, v in t["params"].items():
            ok = ok and k in params and _compare(v, params[k])
        if ok:
            out.append(t)
    return out

def act_suite(name, tests, simulators = ["rtl", "sail"], known_failures = {}):
    """Makes the targets for every test in `tests`.

    For each test `T`: `T` is the self-checking ELF; `T_<sim>_run` is
    its result file on each simulator; `T_<sim>_test` passes when that
    file says PASS. `<name>_<sim>_results` collects the result files of
    a simulator, and `<name>_<sim>_tests` is a test suite of its tests.

    Args:
      name: the suite's name.
      tests: the tests, from `select_tests`.
      simulators: the simulators to run on: `rtl`, `sail` or both.
      known_failures: tests the device is known to fail, mapped to the
        URL of the issue that tracks each. Their `T_rtl_test` passes
        while the run fails, and fails once the run passes, so that the
        entry is removed with the fix. The report still counts them as
        failures.
    """
    for sim in simulators:
        native.test_suite(
            name = "%s_%s_tests" % (name, sim),
            tests = ["%s_%s_test" % (t["name"], sim) for t in tests],
        )
        native.filegroup(
            name = "%s_%s_results" % (name, sim),
            srcs = ["%s_%s_run" % (t["name"], sim) for t in tests],
        )
    for t in tests:
        act_elf(
            name = t["name"],
            src = "@riscv_arch_test//:" + t["src"],
            march = t["march"],
            needs_signature = t["needs_signature"],
        )
        for sim in simulators:
            act_run(
                name = "%s_%s_run" % (t["name"], sim),
                elf = ":" + t["name"],
                test_name = t["name"],
                simulator = sim,
            )
            sh_test(
                name = "%s_%s_test" % (t["name"], sim),
                size = "small",
                srcs = ["//act:check.sh"],
                args = [
                    "$(rootpath :%s_%s_run)" % (t["name"], sim),
                    "FAIL" if sim == "rtl" and t["name"] in known_failures else "PASS",
                ],
                data = [":%s_%s_run" % (t["name"], sim)],
            )

def _act_report_impl(ctx):
    manifest = ctx.actions.declare_file(ctx.label.name + ".manifest.tsv")
    lines = []
    inputs = []
    for i, elf in enumerate(ctx.attr.elfs):
        info = elf[ActElfInfo]
        rtl = ctx.files.rtl_results[i]
        sail = ctx.files.sail_results[i]
        inputs += [rtl, sail]
        if info.results:
            inputs.append(info.results)
        lines.append("\t".join([
            ctx.attr.suites[i],
            ctx.attr.names[i],
            rtl.path,
            sail.path,
            info.results.path if info.results else "-",
            ctx.attr.issues[i] or "-",
        ]))
    ctx.actions.write(manifest, "\n".join(lines) + "\n")
    names = ["results.tex", "suites.tex", "tests.tex", "failures.tex", "cpichart.tex", "report.html", "results.tsv"]
    outs = [ctx.actions.declare_file(n) for n in names]
    args = ctx.actions.args()
    args.add("--manifest", manifest)
    args.add("--stable", ctx.info_file)
    args.add("--volatile", ctx.version_file)
    args.add("--gcc", ctx.file._gcc)
    args.add("--sail", ctx.file._sail)
    args.add("--act-commit", ctx.attr.act_commit)
    args.add("--catalog", str(ctx.attr.catalog))
    args.add("--extensions", ",".join(ctx.attr.extensions))
    for suite, why in ctx.attr.excluded.items():
        args.add("--excluded", "%s=%s" % (suite, why))
    args.add("--out-dir", outs[0].dirname)
    ctx.actions.run(
        executable = ctx.executable._report,
        arguments = [args],
        inputs = depset(
            inputs + [manifest, ctx.info_file, ctx.version_file, ctx.file._sail],
            transitive = [depset(ctx.files._gcc_all)],
        ),
        outputs = outs,
        mnemonic = "ActReport",
        progress_message = "Writing the conformance report data",
    )
    return [DefaultInfo(files = depset(outs))]

act_report = rule(
    implementation = _act_report_impl,
    doc = "Writes the report data, //tools/report's outputs, from every run. " +
          "The outputs land in the package directory, beside the article.",
    attrs = {
        "elfs": attr.label_list(providers = [ActElfInfo], doc = "The tests, from act_elf."),
        "names": attr.string_list(doc = "The tests' names, parallel to elfs."),
        "suites": attr.string_list(doc = "Each test's suite, parallel to elfs."),
        "issues": attr.string_list(doc = "Each test's issue URL or an empty string."),
        "rtl_results": attr.label_list(allow_files = True, doc = "The netlist runs, parallel to elfs."),
        "sail_results": attr.label_list(allow_files = True, doc = "The Sail runs, parallel to elfs."),
        "extensions": attr.string_list(doc = "The extensions the device implements."),
        "excluded": attr.string_dict(doc = "Suites left out by hand, with the reason."),
        "catalog": attr.int(doc = "The number of tests in the catalog, before selection."),
        "act_commit": attr.string(doc = "The riscv-arch-test commit."),
        "_report": attr.label(executable = True, cfg = "exec", default = "//tools/report"),
        "_gcc": attr.label(allow_single_file = True, cfg = "exec", default = "@riscv_gcc//:gcc"),
        "_gcc_all": attr.label(cfg = "exec", default = "@riscv_gcc//:all"),
        "_sail": attr.label(allow_single_file = True, cfg = "exec", default = "@sail_riscv//:sail"),
    },
)

def act_suite_report(name, tests, known_failures, extensions, excluded, catalog, act_commit):
    """Makes `act_report` over the targets `act_suite` made for `tests`.

    Args:
      name: the report target's name.
      tests: the tests given to act_suite.
      known_failures: as given to act_suite.
      extensions: the extensions the device implements.
      excluded: suites left out by hand, mapped to the reason.
      catalog: the number of tests in the catalog.
      act_commit: the riscv-arch-test commit.
    """
    act_report(
        name = name,
        elfs = [":" + t["name"] for t in tests],
        names = [t["name"] for t in tests],
        suites = [t["suite"] for t in tests],
        issues = [known_failures.get(t["name"], "") for t in tests],
        rtl_results = [":%s_rtl_run" % t["name"] for t in tests],
        sail_results = [":%s_sail_run" % t["name"] for t in tests],
        extensions = extensions,
        excluded = excluded,
        catalog = catalog,
        act_commit = act_commit,
    )
