{
  description = "Wallpaper Engine";

  inputs = {
    self.submodules = true;

    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];

        pkgs = import nixpkgs {
          inherit system overlays;
        };

        lib = pkgs.lib;

        nixLib = import ./nix/lib.nix { inherit pkgs; };

        apple-libs = nixLib.env;

        buildScripts = nixLib.build;

        rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        xcodeConfiguration = "Release";

        nativeBuildTools = with pkgs; [
          rustToolchain
          cmake
          ninja
          pkg-config
          python3
          jq
        ];

        engineLibraries = with pkgs; [
          libiconv
          eigen
          nlohmann_json
          glslang.dev
          glslang.out
          spirv-tools.dev
          spirv-tools
          argparse
          quickjs-ng.dev
          quickjs-ng
          glm
          lz4.dev
          lz4
          ffmpeg.dev
          ffmpeg
          shaderc
          vulkan-headers
          vulkan-loader
          moltenvk
          vulkan-tools
          vulkan-validation-layers

          apple-sdk_26
        ];

      in
      lib.optionalAttrs pkgs.stdenv.isDarwin {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = nativeBuildTools;
          buildInputs = engineLibraries;
        };

        packages.default = pkgs.stdenv.mkDerivation {
          pname = "wallpaper-engine";

          version = "0.1.1";

          src = ./.;

          nativeBuildInputs = nativeBuildTools ++ [ pkgs.rustPlatform.cargoSetupHook ];
          buildInputs = engineLibraries;

          dontConfigure = true;
          dontFixup = true;

          cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };

          buildPhase = ''
            runHook preBuild

            export GIT_SHORT_COMMIT=${self.shortRev or self.dirtyShortRev}
            ${apple-libs.fakeHomeSetupScript}

            cargo build --workspace --release

            cargo build \
                --release \
                -p wallpaper-bridge \
                --bin uniffi-bindgen

            ${apple-libs.xcodePreflightScript}
            ${apple-libs.resolveDeveloperDirScript}

            export SDKROOT=$(/usr/bin/xcrun --sdk macosx --show-sdk-path)

            mkdir -p \
                app/WallpaperEngine/Bridge/Generated

            ./target/release/uniffi-bindgen \
                generate \
                --library target/release/libwallpaper_bridge.a \
                --language swift \
                --no-format \
                --out-dir app/WallpaperEngine/Bridge/Generated

            export BUILD_DIR="$TMPDIR/build"
            mkdir -p "$BUILD_DIR"

            /usr/bin/xcodebuild \
                -project app/WallpaperEngine.xcodeproj \
                -scheme WallpaperEngine \
                -configuration ${xcodeConfiguration} \
                -derivedDataPath "$BUILD_DIR" \
                OBJROOT="$BUILD_DIR/Intermediates" \
                SYMROOT="$BUILD_DIR/Products" \
                CONFIGURATION_BUILD_DIR="$BUILD_DIR/${xcodeConfiguration}" \
                OTHER_SWIFT_FLAGS='$(inherited) -Xcc -fmodule-map-file=$(SRCROOT)/WallpaperEngine/Bridge/Generated/WallpaperBridgeFFI.modulemap -disable-sandbox' \
                build

            APP="$BUILD_DIR/${xcodeConfiguration}/Wallpaper Engine.app"
            ${buildScripts.packageBundleScript}

            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall

            mkdir -p "$out/Applications"

            cp -R \
              "$BUILD_DIR/${xcodeConfiguration}/Wallpaper Engine.app" \
              "$out/Applications/"

            runHook postInstall
          '';

          doInstallCheck = true;

          installCheckPhase = ''
            runHook preInstallCheck

            APP="$out/Applications/Wallpaper Engine.app"
            ${buildScripts.validateBundleScript}

            runHook postInstallCheck
          '';
        };
      }
    );
}
