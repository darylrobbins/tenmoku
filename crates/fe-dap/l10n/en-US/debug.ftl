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
