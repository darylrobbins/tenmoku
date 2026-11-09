# fe-net's messages: why a request failed.

net-host-not-allowed = { $host } is not a host this client may reach
net-scheme = only https URLs are allowed (http only to 127.0.0.1 or [::1])
net-connect = couldn't connect: { $detail }
net-tls = TLS failed: { $detail }
net-io = the connection broke: { $detail }
net-http = HTTP error: { $detail }
net-timeout = timed out
net-invalid-url = invalid URL { $url }: { $error }
net-url-no-host = the URL { $url } has no host
net-url-user-name = the URL { $url } has a user name, which isn't sent
net-url-invalid-port = the URL { $url } has an invalid port
net-idle = nothing was received for { $idle }

net-grammar-not-released = this grammar isn't released yet
net-grammar-invalid-hash = the recorded hash { $hash } isn't a SHA-256
net-grammar-hash-mismatch = the download's SHA-256 is { $actual }, not the recorded { $expected }
net-grammar-too-large =
    { $max_bytes ->
        [one] the download is over the limit of { $max_bytes } bytes
       *[other] the download is over the limit of { $max_bytes } bytes
    }
net-grammar-host-not-allowed = { $host } is not a host this download may reach
net-grammar-too-many-redirects =
    { $max ->
        [one] the download redirected more than { $max } times
       *[other] the download redirected more than { $max } times
    }
net-grammar-bad-redirect = the download redirected to { $location }, which can't be followed
net-grammar-status = the server answered { $status }
