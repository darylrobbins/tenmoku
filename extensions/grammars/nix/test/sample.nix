# A flake-style package definition.
{ lib
, stdenv
, fetchFromGitHub
, bash
, enableDocs ? true
, ...
}:

/* Block comment: the version and
   the build inputs. */
let
  version = "1.2.3";
  count = 42;
  ratio = 0.75;
  names = [ "alpha" "beta" ];
  greet = name: "Hello, ${name}!\n\tTab \"quoted\" \${literal}";
  add = { a, b ? 1 }: a + b;
  total = builtins.foldl' (x: y: x + y) 0 [ 1 2 3 ];
  path = ./src/main.c;
  home = ~/projects;
  channel = <nixpkgs>;
  link = https://example.com/archive.tar.gz;
in
stdenv.mkDerivation rec {
  pname = "example";
  inherit version;
  src = fetchFromGitHub {
    owner = "example";
    repo = pname;
    rev = "v${version}";
    hash = lib.fakeHash;
  };

  buildInputs = [ bash ] ++ lib.optionals enableDocs [ names ];

  buildPhase = ''
    echo "building ${pname}"
    for f in *.c; do
      $CC -c "$f"
    done
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp example $out/bin/
  '';

  meta = with lib; {
    description = if enableDocs then "With docs" else "No docs";
    license = licenses.mit;
    broken = assert count > 0; false;
    platforms = platforms.unix or [ ];
  };

  passthru.tests = map (n: n + "-test") names;
  helper = import ./helper.nix { inherit lib; };
  check = !(count == 42) || (ratio >= 0.5 && builtins.isNull null);
  script = writeShellScript "run" ''
    exec ${bash}/bin/bash "$@"
  '';
  report = runCommand "report" { } ''
    echo done > $out
  '';
  app = writeShellApplication {
    name = "app";
    text = ''
      echo "${pname}"
    '';
  };
}
