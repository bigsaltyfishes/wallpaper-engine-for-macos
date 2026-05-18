{ ... }:
{
  # Host Xcode preflight. The Apple build is inherently impure on the path
  # to a real Xcode install — Tuist, xcodebuild, and UniFFI's iOS
  # cross-compile all shell out to `xcrun`. Rather than hide that, we check
  # it explicitly and fail early with a useful message instead of letting a
  # downstream command die with a confusing error.
  xcodePreflightScript = ''
    set -eu
    if [ ! -d /Applications/Xcode.app/Contents/Developer ] && \
       [ ! -d /Applications/Xcode-beta.app/Contents/Developer ]; then
      echo "error: ClipKitty's Nix build requires a host Xcode install at" >&2
      echo "  /Applications/Xcode.app or /Applications/Xcode-beta.app." >&2
      echo "  Tuist, xcodebuild, and UniFFI's iOS cross-compile all shell" >&2
      echo "  out to /usr/bin/xcrun and need the real Xcode." >&2
      exit 1
    fi
    if [ ! -x /usr/bin/xcrun ]; then
      echo "error: /usr/bin/xcrun is missing; install Xcode command-line tools." >&2
      exit 1
    fi
    if ! /usr/bin/xcrun --sdk macosx --show-sdk-path >/dev/null 2>&1; then
      echo "error: xcrun cannot resolve the macOS SDK path." >&2
      exit 1
    fi
  '';

  # Resolve a usable DEVELOPER_DIR for invocations that need Xcode's full
  # platform tree (iOS SDK, xcodebuild, tuist's generator). Exported as a
  # shell variable rather than baked into the Nix store path because the
  # real location is host-dependent.
  resolveDeveloperDirScript = ''
    DEVELOPER_DIR=""
    for candidate in \
      /Applications/Xcode.app/Contents/Developer \
      /Applications/Xcode-beta.app/Contents/Developer; do
      if [ -d "$candidate" ]; then
        DEVELOPER_DIR="$candidate"
        break
      fi
    done
    if [ -z "$DEVELOPER_DIR" ]; then
      echo "error: no usable Xcode install found on host" >&2
      exit 1
    fi
    export DEVELOPER_DIR
  '';

  fakeHomeSetupScript = ''
      # Tuist/SwiftPM need a writable HOME for their internal caches,
      # session files, and generated plugins. Point them all inside the
      # build directory so nothing escapes the sandbox.
      export HOME=$TMPDIR/build-home
      mkdir -p "$HOME"
      export XDG_STATE_HOME="$HOME/.local/state"
      export XDG_CACHE_HOME="$HOME/.cache"
      mkdir -p "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
  '';
}
