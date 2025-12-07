{ pkgs, lib, config, inputs, ... }:

let
  koreader = pkgs.callPackage ./packages/koreader.nix {};
  cargo-debugger = pkgs.rustPlatform.buildRustPackage {
    pname = "cargo-debugger";
    version = "0.1.0";
    src = pkgs.fetchFromGitHub {
      owner = "jkelleyrtp";
      repo = "cargo-debugger";
      rev = "master";
      sha256 = "sha256-5LJvGy6jZLsN3IhgWktLKvH8seAvee0cAH4Rs+1Wmuk=";
    };

    cargoHash = "sha256-19cm5wVs6z+XuBcIwqWOgoLY6lP6G1jUM68mmRNGf5U=";

    buildInputs = with pkgs; [
      openssl
      pkg-config
    ] ++ lib.optionals stdenv.isDarwin [
      darwin.apple_sdk.frameworks.Security
    ];
  };

  koreaderFrontendPath = if pkgs.stdenv.isDarwin
    then "${koreader}/Applications/KOReader.app/Contents/koreader/frontend"
    else "${koreader}/lib/koreader/frontend";

  luarcConfig = ''
    {
      "$schema": "https://raw.githubusercontent.com/sumneko/vscode-lua/master/setting/schema.json",
      "diagnostics.globals": [
        "G_reader_settings"
      ],
      "workspace.library": [
        "''${3rd}/luassert/library",
        "''${3rd}/busted/library",
        "${koreaderFrontendPath}"
      ],
      "runtime.version": "LuaJIT",
      "diagnostics.neededFileStatus": {
        "codestyle-check": "Any"
      }
    }
  '';

  pkgs-unstable = import inputs.nixpkgs-unstable {
    system = pkgs.stdenv.system;
  };
in {
  # https://devenv.sh/packages/
  packages = [koreader cargo-debugger] ++ (with pkgs; [ 
    cargo-flamegraph
    clang
    gcc
    git
    lua-language-server
    luajitPackages.busted
    mdbook
    mdbook-admonish
    pkgs-unstable.poetry
    python313Full
    python313Packages.tkinter
    sqlx-cli
    sshpass
    pkg-config
  ] ++ lib.optionals (!stdenv.isDarwin) [
    mold-wrapped
  ] ++ lib.optionals stdenv.isDarwin [
    libiconv
    darwin.apple_sdk.frameworks.SystemConfiguration
  ]);

  # Add generated files
  files = {
    ".cargo/config.toml".text = ''
      [target.x86_64-unknown-linux-gnu]
      linker = "${pkgs.clang}/bin/clang"
      rustflags = ["-C", "link-arg=--ld-path=${pkgs.mold-wrapped}/bin/mold"]

      [target.aarch64-apple-darwin]
      linker = "${pkgs.clang}/bin/clang"
    '';

    ".luarc.json".text = luarcConfig;
    "frontend/.luarc.json".text = luarcConfig;
  };

  # https://devenv.sh/languages/
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.91.1";
  };

  # Enable cachix
  cachix = {
    enable = true;
    push = "rakuyomi";
  };

  enterShell = ''
    cargo fetch --manifest-path="$DEVENV_ROOT/backend/Cargo.toml"
  '';

  scripts = {
    check-format.exec = "cd $DEVENV_ROOT/backend && cargo fmt --check";
    check-lint.exec = ''
      cd $DEVENV_ROOT/backend && cargo clippy -- -D warnings
      cd $DEVENV_ROOT && python3 ci/lua-language-server-check.py frontend/
    '';
    fix-rust-format.exec = "cd $DEVENV_ROOT/backend && cargo fmt --all";
    fix-rust-lint.exec = "cd $DEVENV_ROOT/backend && cargo clippy --fix --allow-dirty -- -D warnings";
    dev.exec = "cd $DEVENV_ROOT && . tools/run-koreader-with-plugin.sh";
    debug.exec = "cd $DEVENV_ROOT && . tools/run-koreader-with-plugin.sh --debug";
    docs.exec = "cd $DEVENV_ROOT/docs && exec mdbook serve --open";
    prepare-sql-queries.exec = "cd $DEVENV_ROOT && . tools/prepare-sqlx-queries.sh";
    remote-install.exec = "cd $DEVENV_ROOT && python3 tools/install-into-remote-koreader.py";
    remote-ssh.exec = ''
      sshpass -p "" ssh \
      -p "$REMOTE_KOREADER_SSH_PORT" \
      -o StrictHostKeyChecking=no \
      "root@$REMOTE_KOREADER_HOST" "$@"
    '';
    test-frontend.exec = "cd $DEVENV_ROOT && busted -C frontend/rakuyomi.koplugin .";
    test-e2e.exec = ''
      cd $DEVENV_ROOT/e2e-tests && \
      poetry env use $(which python) && \
      poetry install --no-root && \
      poetry run pytest $@
    '';
  };

  # See full reference at https://devenv.sh/reference/options/
}
