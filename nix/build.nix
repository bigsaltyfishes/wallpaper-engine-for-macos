{ pkgs }:

let
  bundleFunctions = ''
    list_linked_libraries() {
        otool -L "$1" 2>/dev/null | awk 'NR > 1 { print $1 }'
    }

    list_rpaths() {
        otool -l "$1" 2>/dev/null | awk '/cmd LC_RPATH/ { getline; getline; print $2 }'
    }

    system_library_path() {
        case "$1" in
            /nix/store/*/System/Library/Frameworks/*)
                echo "/System/Library/Frameworks/''${1#*/System/Library/Frameworks/}"
                ;;
            /nix/store/*/System/Library/PrivateFrameworks/*)
                echo "/System/Library/PrivateFrameworks/''${1#*/System/Library/PrivateFrameworks/}"
                ;;
            /nix/store/*/usr/lib/swift/*)
                echo "/usr/lib/swift/''${1#*/usr/lib/swift/}"
                ;;
            *)
                case "$(basename "$1")" in
                    libc++.1.dylib|libc++.1.0.dylib)
                        echo "/usr/lib/libc++.1.dylib"
                        ;;
                    libobjc.A.dylib)
                        echo "/usr/lib/libobjc.A.dylib"
                        ;;
                    libSystem.B.dylib)
                        echo "/usr/lib/libSystem.B.dylib"
                        ;;
                    *)
                        return 1
                        ;;
                esac
                ;;
        esac
    }

    is_gnu_iconv() {
        case "$1" in
            /nix/store/*-libiconv-1.*)
                ;;
            *)
                return 1
                ;;
        esac

        case "$(basename "$1")" in
            libiconv.2.dylib|libiconv.dylib)
                ;;
            *)
                return 1
                ;;
        esac
    }

    bundled_library_name() {
        if is_gnu_iconv "$1"; then
            echo "libgnuiconv.2.dylib"
            return
        fi

        basename "$1"
    }

    bundled_library_path() {
        echo "$BUNDLED_LIBRARY_PATH/$(bundled_library_name "$1")"
    }

    bundled_library_file() {
        echo "$FRAMEWORKS/$(bundled_library_name "$1")"
    }

    rewrite_library_path() {
        case "$1" in
            /nix/store/*)
                system_library_path "$1" || bundled_library_path "$1"
                ;;
            *)
                return 1
                ;;
        esac
    }

    should_keep_rpath() {
        case "$1" in
            @executable_path/../Frameworks|@executable_path/../Frameworks/)
                echo "@executable_path/../Frameworks"
                ;;
            @executable_path/../PlugIns|@executable_path/../PlugIns/)
                echo "@executable_path/../PlugIns"
                ;;
            *)
                return 1
                ;;
        esac
    }

    normalize_rpaths() {
        local file="$1"
        local kept_rpaths="$TMPDIR/kept-rpaths"
        local normalized_rpath
        local rpath

        : > "$kept_rpaths"

        while IFS= read -r rpath; do
            [ -n "$rpath" ] || continue

            normalized_rpath="$(should_keep_rpath "$rpath")" || continue
            printf '%s\n' "$normalized_rpath" >> "$kept_rpaths"
        done < <(list_rpaths "$file")

        while IFS= read -r rpath; do
            [ -n "$rpath" ] || continue

            install_name_tool -delete_rpath "$rpath" "$file" 2>/dev/null || true
        done < <(list_rpaths "$file")

        while IFS= read -r rpath; do
            [ -n "$rpath" ] || continue

            install_name_tool -add_rpath "$rpath" "$file"
        done < <(sort -u "$kept_rpaths")
    }

    set_library_id() {
        local file="$1"

        case "$file" in
            *.dylib)
                install_name_tool \
                    -id "$(bundled_library_path "$file")" \
                    "$file"
                ;;
        esac
    }

    copy_library() {
        local source="$1"
        local target

        system_library_path "$source" >/dev/null && return 0

        target="$(bundled_library_file "$source")"

        [ -f "$source" ] || return 0

        if [ ! -e "$target" ]; then
            echo "Bundling $(basename "$source")"

            cp -L "$source" "$target"
            chmod u+w "$target"
            set_library_id "$target"
            printf '%s\n' "$target" >> "$NEXT_LIBRARY_QUEUE"
        fi
    }

    patch_linked_libraries() {
        local file="$1"
        local dependency
        local replacement

        chmod u+w "$file"
        normalize_rpaths "$file"

        while IFS= read -r dependency; do
            replacement="$(rewrite_library_path "$dependency")" || continue

            if [ "$dependency" != "$replacement" ]; then
                install_name_tool \
                    -change "$dependency" "$replacement" \
                    "$file"
            fi
        done < <(list_linked_libraries "$file")

        set_library_id "$file"
    }
  '';

  verifyBundleFunction = ''
    verify_bundle() {
        local app="$1"
        local fail=0
        local linked_libs
        local lib
        local file
        local rpath
        local rpaths

        if [[ ! -d "$app" ]]; then
            echo "FAIL app bundle not found: $app"
            return 1
        fi

        if [[ ! -x "$app/Contents/MacOS/Wallpaper Engine" ]]; then
            echo "FAIL missing or non-executable app binary: $app/Contents/MacOS/Wallpaper Engine"
            fail=1
        fi

        while IFS= read -r file; do
            if ! linked_libs=$(otool -L "$file" 2>/dev/null); then
                continue
            fi

            while IFS= read -r lib; do
                case "$lib" in
                    /nix/store/*)
                        echo "FAIL $file -> $lib"
                        fail=1
                        ;;
                esac
            done < <(awk 'NR > 1 { print $1 }' <<<"$linked_libs")
        done < <(find "$app" -type f \( -name '*.dylib' -o -name '*.so' \
            -o -perm -100 -o -perm -010 -o -perm -001 \) \
            ! -name '*.sh' ! -name '*.json' ! -name '*.plist')

        local gnu_iconv="$app/Contents/Frameworks/libgnuiconv.2.dylib"
        if [[ -f "$gnu_iconv" ]]; then
            for lib in _libiconv _libiconv_open _libiconv_close; do
                if ! nm -gU "$gnu_iconv" 2>/dev/null | awk '{ print $NF }' | grep -Fxq "$lib"; then
                    echo "FAIL $gnu_iconv does not export $lib"
                    fail=1
                fi
            done
        fi

        while IFS= read -r file; do
            if nm -u "$file" 2>/dev/null | grep -Eq '^_libiconv($|_open$|_close$)' \
                && ! otool -L "$file" 2>/dev/null | awk 'NR > 1 { print $1 }' \
                    | grep -Fxq '@executable_path/../Frameworks/libgnuiconv.2.dylib'; then
                echo "FAIL $file imports GNU libiconv symbols without linking libgnuiconv.2.dylib"
                fail=1
            fi
        done < <(find "$app" -type f \( -name '*.dylib' -o -name '*.so' \
            -o -perm -100 -o -perm -010 -o -perm -001 \) \
            ! -name '*.sh' ! -name '*.json' ! -name '*.plist')

        while IFS= read -r file; do
            if ! rpaths=$(otool -l "$file" 2>/dev/null \
                | awk '/cmd LC_RPATH/ { getline; getline; print $2 }'); then
                continue
            fi

            while IFS= read -r rpath; do
                if [[ -n "$rpath" ]]; then
                    echo "FAIL $file has duplicate LC_RPATH: $rpath"
                    fail=1
                fi
            done < <(sort <<<"$rpaths" | uniq -d)

            while IFS= read -r rpath; do
                case "$rpath" in
                    ""|@executable_path/../Frameworks|@executable_path/../PlugIns)
                        ;;
                    *)
                        echo "FAIL $file has unsupported LC_RPATH: $rpath"
                        fail=1
                        ;;
                esac
            done <<<"$rpaths"
        done < <(find "$app" -type f \( -name '*.dylib' -o -name '*.so' \
            -o -perm -100 -o -perm -010 -o -perm -001 \) \
            ! -name '*.sh' ! -name '*.json' ! -name '*.plist')

        local icd="$app/Contents/Resources/MoltenVK_icd.json"
        local moltenvk="$app/Contents/Frameworks/libMoltenVK.dylib"
        if [[ ! -f "$icd" ]]; then
            echo "FAIL missing MoltenVK ICD JSON: $icd"
            fail=1
        elif ! lib=$(jq -er '.ICD.library_path | strings' "$icd"); then
            echo "FAIL $icd missing string .ICD.library_path"
            fail=1
        elif [[ "$lib" != "libMoltenVK.dylib" ]]; then
            echo "FAIL $icd has unsupported .ICD.library_path: $lib"
            fail=1
        elif [[ ! -f "$moltenvk" ]]; then
            echo "FAIL $icd references missing library: $lib"
            fail=1
        fi

        while IFS= read -r file; do
            echo "FAIL MoltenVK dylib must be in Frameworks, not Resources: $file"
            fail=1
        done < <(find "$app/Contents/Resources" -maxdepth 1 -name 'libMoltenVK*.dylib' -print)

        /usr/bin/codesign --verify --deep --strict --verbose=2 "$app" || fail=1

        if [[ $fail -eq 0 ]]; then
            echo "Bundle verified."
        else
            echo "Bundle verification failed."
            return 1
        fi
    }
  '';
in
{
  packageBundleScript = ''
    MAIN_BINARY_NAME="Wallpaper Engine"
    MAIN_BINARY="$APP/Contents/MacOS/$MAIN_BINARY_NAME"
    FRAMEWORKS="$APP/Contents/Frameworks"
    RESOURCES="$APP/Contents/Resources"
    BUNDLED_LIBRARY_PATH="@executable_path/../Frameworks"

    mkdir -p "$FRAMEWORKS" "$RESOURCES"

    ${bundleFunctions}

    echo "Bundling linked dylibs"

    LIBRARY_QUEUE="$TMPDIR/dylib-queue"
    NEXT_LIBRARY_QUEUE="$TMPDIR/dylib-next-queue"

    : > "$LIBRARY_QUEUE"
    : > "$NEXT_LIBRARY_QUEUE"

    printf '%s\n' "$MAIN_BINARY" >> "$LIBRARY_QUEUE"
    copy_library "${pkgs.moltenvk.out}/lib/libMoltenVK.dylib"
    sort -u "$NEXT_LIBRARY_QUEUE" >> "$LIBRARY_QUEUE"

    while [ -s "$LIBRARY_QUEUE" ]; do
        : > "$NEXT_LIBRARY_QUEUE"

        while IFS= read -r file; do
            [ -f "$file" ] || continue

            while IFS= read -r dependency; do
                case "$dependency" in
                    /nix/store/*)
                        copy_library "$dependency"
                        ;;
                esac
            done < <(list_linked_libraries "$file")
        done < "$LIBRARY_QUEUE"

        sort -u "$NEXT_LIBRARY_QUEUE" > "$LIBRARY_QUEUE"
    done

    patch_linked_libraries "$MAIN_BINARY"

    while IFS= read -r file; do
        patch_linked_libraries "$file"
    done < <(
        find "$FRAMEWORKS" "$RESOURCES" \
            -type f \
            \( -name '*.dylib' -o -name '*.so' \) \
            | sort
    )

    cat \
        "${pkgs.moltenvk.out}/share/vulkan/icd.d/MoltenVK_icd.json" \
        | jq \
        '.ICD.library_path = "libMoltenVK.dylib"' \
        > "$RESOURCES/MoltenVK_icd.json"

    /usr/bin/codesign \
        --force \
        --deep \
        --sign - \
        "$APP"
  '';

  validateBundleScript = ''
    ${verifyBundleFunction}

    verify_bundle "$APP"
  '';
}
