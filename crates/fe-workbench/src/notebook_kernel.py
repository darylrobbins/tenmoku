# Tenmoku's notebook kernel: runs `# %%` cells of one Python file in one
# persistent namespace, speaking JSON lines with the editor. Standard library
# only, for any Python 3.8 or later. The protocol is described in
# notebook_kernel.rs, which embeds this file and runs it as
# `<python> -u <this file> <config json>`.
#
# File descriptors: the protocol reads the original stdin and writes the
# original stdout, both duplicated to private descriptors at startup. fd 0
# then reads /dev/null, so a subprocess can't eat the protocol, and fds 1 and
# 2 write pipes this process reads, so a subprocess's or C extension's output
# comes back as `stream` messages too. The original stderr is kept for this
# script's own last words, should it fail.

import ast
import io
import json
import linecache
import os
import platform
import select
import signal
import sys
import threading
import time
import traceback
import types

CONFIG = json.loads(sys.argv[1])
OUTPUT_CAP = int(CONFIG["output_cap"])
MAX_LINE = int(CONFIG["max_line"])
# A batch of stream text goes out once it is this big, or this old.
BATCH_BYTES = 4096
BATCH_SECONDS = 0.05
# The most text one message carries: a longer write goes as several.
CHUNK = 65536
# The most a result's repr, an error's value or a traceback entry keeps.
FIELD_CAP = OUTPUT_CAP

KERNEL_FILE = os.path.normcase(os.path.abspath(__file__))
NO_INPUT = "input() is not supported in notebook cells: there is no stdin"


class State:
    def __init__(self):
        # The id of the cell running, or None between cells.
        self.cell = None
        # Whether user code is running, so SIGINT may interrupt it.
        self.executing = False
        # Whether the main thread is writing a protocol line, which an
        # interrupt must not cut short.
        self.in_proto = False
        self.pending_interrupt = False


STATE = State()


def utf8(text):
    """`text` with what can't be UTF-8 (lone surrogates) replaced."""
    return text.encode("utf-8", "replace").decode("utf-8")


def clip(text, cap):
    """`text` with what can't be UTF-8 replaced, at most `cap` characters."""
    if not isinstance(text, str):
        text = str(text)
    text = utf8(text)
    if len(text) > cap:
        text = text[:cap] + "\n[%d characters dropped]" % (len(text) - cap)
    return text


class Proto:
    """The protocol's writing end: one whole JSON line at a time."""

    def __init__(self, fd):
        self.fd = fd
        self.lock = threading.Lock()

    def send(self, message):
        data = (json.dumps(message, ensure_ascii=True) + "\n").encode("ascii")
        main = threading.current_thread() is threading.main_thread()
        with self.lock:
            if main:
                STATE.in_proto = True
            try:
                view = memoryview(data)
                while view:
                    view = view[os.write(self.fd, view):]
            except OSError:
                # The editor is gone.
                os._exit(1)
            finally:
                if main:
                    STATE.in_proto = False
        if main and STATE.pending_interrupt and STATE.executing:
            STATE.pending_interrupt = False
            raise KeyboardInterrupt


class Batcher:
    """Stream text, batched, capped at OUTPUT_CAP bytes per cell run (and
    per stretch between cells), its lines split at MAX_LINE characters.

    Text belongs to the cell running when it was written (None between
    cells), whenever it's flushed; the run changes only under the lock, in
    `begin` and `end`, so the counters are always the current run's."""

    def __init__(self, proto):
        self.proto = proto
        self.lock = threading.RLock()
        # (cell, name, [text, ...]), oldest first.
        self.parts = []
        self.size = 0
        self.reset()

    def reset(self):
        self.sent = 0
        self.dropped = 0
        self.line = {"stdout": 0, "stderr": 0}

    def write(self, name, text):
        if not text:
            return
        with self.lock:
            cell = STATE.cell
            data = text.encode("utf-8", "replace")
            room = max(OUTPUT_CAP - self.sent, 0)
            if len(data) > room:
                # Cut at a character's start, counting what's dropped.
                text = data[:room].decode("utf-8", "ignore")
                kept = len(text.encode("utf-8"))
                self.dropped += len(data) - kept
                data = data[:kept]
            else:
                text = data.decode("utf-8")
            self.sent += len(data)
            if text:
                text = self.split(name, text)
                last = self.parts[-1] if self.parts else None
                if last and last[0] == cell and last[1] == name:
                    last[2].append(text)
                else:
                    self.parts.append((cell, name, [text]))
                self.size += len(data)
            full = self.size >= BATCH_BYTES
        if full:
            self.flush()

    def split(self, name, text):
        """`text` with a line break after each MAX_LINE characters of a line."""
        run = self.line[name]
        if "\n" not in text and run + len(text) <= MAX_LINE:
            self.line[name] = run + len(text)
            return text
        out = []
        for piece in text.split("\n"):
            if out:
                out.append("\n")
                run = 0
            while run + len(piece) > MAX_LINE:
                take = MAX_LINE - run
                out.append(piece[:take])
                out.append("\n")
                piece = piece[take:]
                run = 0
            out.append(piece)
            run += len(piece)
        self.line[name] = run
        return "".join(out)

    def flush(self):
        with self.lock:
            parts, self.parts, self.size = self.parts, [], 0
            for cell, name, texts in parts:
                text = "".join(texts)
                for start in range(0, len(text), CHUNK):
                    self.send(cell, name, text[start : start + CHUNK])

    def send(self, cell, name, text):
        self.proto.send({"id": cell, "kind": "stream", "name": name, "text": text})

    def finish_run(self):
        """Flushes the run's text and notes what it dropped."""
        self.flush()
        if self.dropped:
            note = "\n[output truncated: %d bytes dropped]\n" % self.dropped
            self.send(STATE.cell, "stderr", note)
        self.reset()

    def begin(self, cell):
        """Ends the stretch between cells, and starts `cell`'s run."""
        with self.lock:
            self.finish_run()
            STATE.cell = cell

    def end(self):
        """Ends the cell's run: what's written next is between cells."""
        with self.lock:
            self.finish_run()
            STATE.cell = None


class Out(io.TextIOBase):
    """sys.stdout or sys.stderr: writes become `stream` messages."""

    def __init__(self, name, batcher):
        self._name = name
        self._batcher = batcher

    @property
    def name(self):
        return "<%s>" % self._name

    @property
    def encoding(self):
        return "utf-8"

    @property
    def errors(self):
        return "replace"

    def writable(self):
        return True

    def isatty(self):
        return False

    def write(self, text):
        if not isinstance(text, str):
            raise TypeError("write() argument must be str, not %s" % type(text).__name__)
        self._batcher.write(self._name, text)
        return len(text)

    def flush(self):
        self._batcher.flush()


class In(io.TextIOBase):
    """sys.stdin: there is none."""

    @property
    def name(self):
        return "<stdin>"

    def readable(self):
        return True

    def read(self, size=-1):
        raise EOFError(NO_INPUT)

    def readline(self, size=-1):
        raise EOFError(NO_INPUT)


def on_sigint(signum, frame):
    if not STATE.executing:
        return
    if STATE.in_proto:
        STATE.pending_interrupt = True
        return
    raise KeyboardInterrupt


class Capture:
    """Points `fd` at a pipe whose text a thread hands to the batcher as
    stream `name`."""

    def __init__(self, fd, name, batcher):
        self.read, write = os.pipe()
        os.dup2(write, fd)
        os.close(write)
        self.name = name
        self.batcher = batcher
        self.lock = threading.Lock()
        self.pending = b""
        # Where select works on pipes, a cell's end can take what its
        # subprocesses wrote before it ended; elsewhere the thread does.
        self.pollable = os.name == "posix"
        if self.pollable:
            os.set_blocking(self.read, False)
        threading.Thread(target=self.pump, name="fd%d" % fd, daemon=True).start()

    def pump(self):
        while True:
            if self.pollable:
                select.select([self.read], [], [], BATCH_SECONDS)
                if not self.take():
                    return
                continue
            try:
                data = os.read(self.read, CHUNK)
            except OSError:
                return
            if not data:
                return
            with self.lock:
                self.feed(data)

    def take(self):
        """Reads what's waiting, a bounded amount: False once the pipe has
        closed."""
        with self.lock:
            for _ in range(16):
                try:
                    data = os.read(self.read, CHUNK)
                except BlockingIOError:
                    return True
                except OSError:
                    return False
                if not data:
                    return False
                self.feed(data)
        return True

    def drain(self):
        if self.pollable:
            self.take()

    def feed(self, data):
        pending = self.pending + data
        try:
            text = pending.decode("utf-8")
            self.pending = b""
        except UnicodeDecodeError as error:
            if error.reason == "unexpected end of data" and error.start >= len(pending) - 3:
                # A character cut in two: its start waits for the rest.
                text = pending[: error.start].decode("utf-8", "replace")
                self.pending = pending[error.start :]
            else:
                text = pending.decode("utf-8", "replace")
                self.pending = b""
        self.batcher.write(self.name, text)


def flusher(batcher):
    while True:
        time.sleep(BATCH_SECONDS)
        batcher.flush()


class Lines:
    """The source of every cell run so far, by file line, for tracebacks:
    the file on disk may not hold what ran."""

    def __init__(self):
        self.files = {}

    def remember(self, file, code, first_line):
        lines = self.files.setdefault(file, [])
        rows = code.split("\n")
        end = first_line - 1 + len(rows)
        while len(lines) < end:
            lines.append("\n")
        for offset, row in enumerate(rows):
            lines[first_line - 1 + offset] = row + "\n"
        size = sum(len(line) for line in lines)
        # mtime None: linecache.checkcache leaves the entry alone.
        linecache.cache[file] = (size, None, lines, file)


def is_kernel(filename):
    try:
        return os.path.normcase(os.path.abspath(filename)) == KERNEL_FILE
    except (TypeError, ValueError):
        return False


def strip_kernel(te, seen):
    """Drops this script's frames from `te` and every exception it chains."""
    if te is None or id(te) in seen:
        return
    seen.add(id(te))
    te.stack = traceback.StackSummary.from_list(
        [frame for frame in te.stack if not is_kernel(frame.filename)]
    )
    strip_kernel(te.__cause__, seen)
    strip_kernel(te.__context__, seen)
    for sub in getattr(te, "exceptions", None) or ():
        strip_kernel(sub, seen)


def error_message(cell, error):
    te = traceback.TracebackException(type(error), error, error.__traceback__)
    strip_kernel(te, set())
    entries = [clip(chunk.rstrip("\n"), FIELD_CAP) for chunk in te.format()]
    try:
        evalue = str(error)
    except BaseException:
        evalue = "<unprintable %s>" % type(error).__name__
    return {
        "id": cell,
        "kind": "error",
        "ename": type(error).__name__,
        "evalue": clip(evalue, FIELD_CAP),
        "traceback": entries,
    }


class Kernel:
    def __init__(self, proto, batcher, captures, file):
        self.proto = proto
        self.batcher = batcher
        self.captures = captures
        self.lines = Lines()
        self.module = types.ModuleType("__main__")
        self.module.__dict__["__builtins__"] = __builtins__
        self.namespace = self.module.__dict__
        self.place(file)
        sys.modules["__main__"] = self.module

    def place(self, file):
        self.namespace["__file__"] = file
        sys.argv = [file]
        folder = os.path.dirname(file)
        if sys.path:
            sys.path[0] = folder
        else:
            sys.path.append(folder)

    def end_output(self):
        """Sends the run's last output, its subprocesses' included."""
        for capture in self.captures:
            capture.drain()
        self.batcher.end()

    def run(self, message):
        cell = message["id"]
        self.batcher.begin(cell)
        ok = True
        STATE.pending_interrupt = False
        # From here an interrupt is the cell's: `busy` tells the editor so,
        # and an interrupt it held for the cell comes now.
        STATE.executing = True
        try:
            self.proto.send({"id": cell, "kind": "busy"})
            file = message["file"]
            code = message["code"]
            first_line = max(int(message.get("line", 1)), 1)
            if file != self.namespace.get("__file__"):
                self.place(file)
            self.lines.remember(file, code, first_line)
            source = "\n" * (first_line - 1) + code
            tree = compile(source, file, "exec", ast.PyCF_ONLY_AST, dont_inherit=True)
            last = None
            if tree.body and isinstance(tree.body[-1], ast.Expr):
                last = ast.Expression(tree.body.pop().value)
            body = compile(tree, file, "exec", dont_inherit=True)
            last = compile(last, file, "eval", dont_inherit=True) if last else None
            try:
                exec(body, self.namespace)
                value = eval(last, self.namespace) if last else None
                text = None if value is None else repr(value)
            finally:
                STATE.executing = False
            if value is not None:
                self.namespace["_"] = value
            self.end_output()
            if text is not None:
                self.proto.send({"id": cell, "kind": "result", "text": clip(text, FIELD_CAP)})
        except BaseException as error:  # noqa: B036 - a cell's SystemExit is its error too
            STATE.executing = False
            ok = False
            self.end_output()
            self.proto.send(error_message(cell, error))
        self.proto.send({"id": cell, "kind": "done", "ok": ok})


def main():
    proto_in = os.dup(0)
    proto_out = os.dup(1)
    last_words = os.dup(2)
    null = os.open(os.devnull, os.O_RDONLY)
    os.dup2(null, 0)
    os.close(null)

    proto = Proto(proto_out)
    batcher = Batcher(proto)
    captures = [Capture(1, "stdout", batcher), Capture(2, "stderr", batcher)]
    sys.stdout = Out("stdout", batcher)
    sys.stderr = Out("stderr", batcher)
    sys.stdin = In()
    threading.Thread(target=flusher, args=(batcher,), name="flusher", daemon=True).start()
    if hasattr(signal, "SIGINT"):
        signal.signal(signal.SIGINT, on_sigint)

    try:
        kernel = Kernel(proto, batcher, captures, CONFIG["file"])
        proto.send({"kind": "ready", "python": platform.python_version()})
        reader = os.fdopen(proto_in, "rb")
        while True:
            line = reader.readline()
            if not line:
                break
            message = json.loads(line.decode("utf-8"))
            if message.get("op") == "exec":
                kernel.run(message)
    except BaseException:
        os.write(last_words, traceback.format_exc().encode("utf-8", "replace"))
        os._exit(1)
    batcher.flush()
    os._exit(0)


main()
