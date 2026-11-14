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
