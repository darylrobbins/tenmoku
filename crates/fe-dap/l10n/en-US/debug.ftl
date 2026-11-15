# fe-dap's messages: why a debug adapter couldn't be started or talked to.

dap-adapter-not-found = the debug adapter { $name } wasn't found
dap-launch-cancelled = starting the debug adapter { $name } was cancelled
dap-launch-stdio = the debug adapter's standard streams couldn't be opened
dap-launch-no-port = the debug adapter { $name } talks over TCP, but none of its arguments has { "${port}" } for the port
dap-launch-exited = the debug adapter { $name } exited before it accepted a connection
dap-launch-connect-timeout = the debug adapter { $name } didn't accept a connection on port { $port } within { $seconds } s

dap-error-transport-io = the connection to the debug adapter broke: { $detail }
dap-error-too-large = the debug adapter sent a message of { $bytes } bytes, over the { $megabytes } MB limit
dap-error-no-content-length = the debug adapter sent a message with no Content-Length
dap-error-bad-header = the debug adapter sent a malformed message header
dap-error-header-too-long = the debug adapter sent a message header that never ended
dap-error-bad-message = the debug adapter sent something that isn't a protocol message: { $why }

# A debug session's errors (src/session.rs): why it ended, or why one of its
# requests got no answer.

dap-session-launch-failed = the debug adapter couldn't be started: { $why }
dap-session-initialize-failed = the debug adapter refused to initialize: { $why }
dap-session-initialize-failed-bare = the debug adapter refused to initialize
dap-session-initialize-timeout = the debug adapter didn't answer initialize within { $seconds } s
dap-session-start-failed = the debug adapter couldn't start the program: { $why }
dap-session-start-failed-bare = the debug adapter couldn't start the program
dap-session-start-timeout = the debug adapter didn't start the program within { $seconds } s
dap-session-configuration-done-timeout = the debug adapter didn't answer configurationDone within { $seconds } s, so the session was ended
dap-session-transport = the debug session ended: { $why }
dap-session-bad-message = the debug adapter sent something that isn't a protocol message

dap-request-failed = the debug adapter couldn't do { $command }: { $why }
dap-request-failed-bare = the debug adapter couldn't do { $command }
dap-request-timeout = the debug adapter didn't answer { $command } within { $seconds } s
dap-request-ended = { $command } wasn't answered: the debug session ended
dap-request-bad-body = the debug adapter answered { $command } with something unexpected: { $why }

# Answers to the adapter's own requests.

dap-start-debugging-refused = child sessions are not supported
dap-run-in-terminal-dropped = the terminal couldn't be started
dap-run-in-terminal-ending = the debug session is ending
dap-reverse-unsupported = { $command } isn't supported

# The protocol log.

dap-log-payloads-warning = This log records whole messages. Environment values and expanded { "${env:…}" } references are hidden, but it may still hold secrets of the program's own (its variables, output and evaluation results) and of the configuration's literal strings. Don't share it without reading it first.
dap-log-no-initialized = the adapter sent no initialized event within { $seconds } s of its launch response: configuration counts as complete, and breakpoints go if it comes later

# Why a debug adapter isn't found, and how to get it (src/registry.rs).

dap-hint-lldb-dap = lldb-dap wasn't found on your login shell's PATH. It comes with LLVM, and with Xcode on macOS: install one, or set debug.adapters.lldb-dap.command to its path.
dap-hint-codelldb = codelldb wasn't found on your login shell's PATH. It comes in the adapter folder of the CodeLLDB extension for VS Code: put that folder on your PATH, or set debug.adapters.codelldb.command to the program's path.
dap-hint-debugpy = No Python was found for debugpy: not .venv in the folder, not $VIRTUAL_ENV, and no python3 on your login shell's PATH. Install Python 3 and debugpy (pip install debugpy), or set debug.adapters.debugpy.command to a Python that has it.
dap-hint-delve = dlv wasn't found on your login shell's PATH. Install Delve with go install github.com/go-delve/delve/cmd/dlv@latest, or set debug.adapters.delve.command to its path.
dap-hint-not-found = { $command } wasn't found on your login shell's PATH, or isn't a program. Install it, or set debug.adapters.{ $name }.command to its path.

# Stops and resumes (src/session.rs, card D2b): requests answered locally,
# and why a debugging control is disabled (its tooltip).

dap-request-stale = { $command } wasn't sent: it refers to a stop that's over
dap-request-not-stopped = { $command } wasn't sent: the program may be running
dap-request-session-owned = { $command } wasn't sent: the debug session sends it itself
dap-control-starting = the program is still starting
dap-control-running = the program is running
dap-control-already-stopped = the program is already stopped
dap-control-resuming = waiting for the debug adapter to answer
dap-control-uncertain = the program may be running: continue or pause it first
dap-control-no-threads = the program reported no threads
dap-control-restarting = the program is restarting
dap-control-ending = the debug session is ending
dap-control-ended = the debug session has ended
