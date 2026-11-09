// The module's own path; a quoted path works too.
module "example.com/tenmoku/sample"

go 1.23.4

toolchain go1.23.5

godebug default=go1.21

godebug (
	panicnil=1
	asynctimerchan=0 // keep the old timers
)

require golang.org/x/sync v0.10.0

require (
	github.com/BurntSushi/toml v1.4.0
	golang.org/x/exp v0.0.0-20240719175910-8a7402abbf56 // indirect
	gopkg.in/check.v1 v1.0.0-20201130134442-10cb98267c6c // indirect
	github.com/old/thing v2.3.1+incompatible
)

tool golang.org/x/tools/cmd/stringer

tool (
	example.com/tenmoku/sample/cmd/gen
	honnef.co/go/tools/cmd/staticcheck
)

exclude golang.org/x/net v0.1.0

exclude (
	github.com/old/thing v2.0.0+incompatible
	example.com/broken v1.1.0
)

replace example.com/local => ../local

replace (
	github.com/upstream/lib v1.2.0 => example.com/fork v1.2.3
	example.com/vendored => ./third_party/vendored
)

// v1.0.0 to v1.0.5 shipped a broken parser.
retract [v1.0.0, v1.0.5]

retract (
	v1.1.0 // published by mistake
	[v0.9.0, v0.9.3]
)

ignore ./node_modules

ignore (
	./testdata/generated
	"./docs/site"
)
