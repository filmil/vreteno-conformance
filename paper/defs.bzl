# SPDX-License-Identifier: Apache-2.0
"""`flat_files`: files from elsewhere, linked into this package."""

def _flat_files_impl(ctx):
    outs = []
    for f in ctx.files.srcs:
        out = ctx.actions.declare_file(f.basename)
        ctx.actions.symlink(output = out, target_file = f)
        outs.append(out)
    return [DefaultInfo(files = depset(outs))]

flat_files = rule(
    implementation = _flat_files_impl,
    doc = "Links each file of `srcs` into this package under its own " +
          "name. latex_document copies a file under its path relative " +
          "to the package, and LaTeX looks beside the master only, so a " +
          "file from another package or repository must be brought here " +
          "first. Two inputs with one name are an error.",
    attrs = {
        "srcs": attr.label_list(
            allow_files = True,
            doc = "The files to link.",
        ),
    },
)
