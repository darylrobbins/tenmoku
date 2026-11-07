# Sample Starlark: a Bazel rule file.
"""Rules for building greeting bundles."""

load("@bazel_skylib//lib:paths.bzl", "paths")
load(":defs.bzl", _helper = "helper")

MAX_NAMES = 16
_DEFAULT_GREETING = "Hello, \"world\"!\n"
Greeter = provider(fields = ["name", "greeting"])
VERSION = (1, 2, 0x1F)
RATIO = 0.75e-2

def _greeting(name, template = "%s says hi", *args, **kwargs):
    """Returns the greeting for a name."""
    if not name:
        fail("name must be set")
    elif len(name) > MAX_NAMES:
        fail("name too long: %d" % len(name))
    else:
        pass
    return template % name

def _bundle_impl(ctx):
    out = ctx.actions.declare_file(ctx.label.name + ".txt")
    lines = []
    for name in ctx.attr.names:
        lines.append(_greeting(name, ctx.attr.template))
    for i, line in enumerate(lines):
        if i == 0 or line.startswith("#"):
            continue
        elif i >= 100:
            break
    ctx.actions.write(out, "\n".join(lines), is_executable = False)
    info = Greeter(name = ctx.label.name, greeting = _DEFAULT_GREETING)
    return [DefaultInfo(files = depset([out])), info]

greeting_bundle = rule(
    implementation = _bundle_impl,
    attrs = {
        "names": attr.string_list(mandatory = True),
        "template": attr.string(default = "%s says hi"),
    },
)

def greeting_macro(name, names = [], visibility = None):
    """Wraps greeting_bundle with defaults."""
    squares = {n: len(n) ** 2 for n in names if n}
    doubled = [x * 2 for x in range(3)]
    pick = lambda a, b: a if a > b and not (a == b) else b
    greeting_bundle(
        name = name,
        names = names + ["extra"],
        visibility = visibility,
    )
    native.filegroup(name = name + "_files", srcs = [":" + name])
    assert_ok = True
    return None

greeting_macro(
    name = "demo",
    names = ["ada", "grace", None],
)

cc_library(
    name = "lib",
    srcs = glob(["src/**/*.cc"]),
    deps = select({
        "//conditions:default": [],
    }),
)
