#!/usr/bin/env bash
set -euo pipefail
set -x

# package-deb.sh
#
# SUMMARY
#
#   Packages a .deb file to be distributed in the APT package manager.
#
# ENV VARS
#
#   $TARGET         a target triple. ex: x86_64-apple-darwin (no default)

TARGET="${TARGET:?"You must specify a target triple, ex: x86_64-apple-darwin"}"

#
# Local vars
#

PROJECT_ROOT="$(pwd)"
ARCHIVE_NAME="vector-$TARGET.tar.gz"
ARCHIVE_PATH="target/artifacts/$ARCHIVE_NAME"
ABSOLUTE_ARCHIVE_PATH="$PROJECT_ROOT/$ARCHIVE_PATH"
PACKAGE_VERSION="$("$PROJECT_ROOT/scripts/version.sh")"

#
# Header
#

echo "Packaging .deb for $ARCHIVE_NAME"
echo "TARGET: $TARGET"

#
# Install dependencies
#

if ! [ -x "$(command -v cargo-deb)" ]; then
  cargo install cargo-deb --version '^1.24.0'
fi

if ! [ -x "$(command -v cmark-gfm)" ]; then
  cd /tmp && \
    git clone https://github.com/github/cmark-gfm && \
    cd cmark-gfm && \
    git checkout 0.29.0.gfm.0 && \
    make
    sudo make install INSTALL_PREFIX=/usr && \
    sudo ldconfig && \
    cd .. && \
    rm -rf cmark-gfm && \
    cmark-gfm --version
fi

#
# Unarchive
#

# Unarchive the tar since cargo deb wants direct access to the files.
td="$(mktemp -d)"
pushd "$td"
tar -xvf "$ABSOLUTE_ARCHIVE_PATH"
mkdir -p "$PROJECT_ROOT/target/$TARGET/release"
mv "vector-$TARGET/bin/vector" "$PROJECT_ROOT/target/$TARGET/release"
popd
rm -rf "$td"

# Display disk space
df -h

#
# Package
#

# Create short plain-text extended description for the package
EXPANDED_LINK_ALIASED="$(cmark-gfm "$PROJECT_ROOT/README.md" --to commonmark)" # expand link aliases
TEXT_BEFORE_FIRST_HEADER="$(sed '/^## /Q' <<< "$EXPANDED_LINK_ALIASED")" # select text before first header
PLAIN_TEXT="$(cmark-gfm --to plaintext <<< "$TEXT_BEFORE_FIRST_HEADER")" # convert to plain text
FORMATTED="$(fmt -uw 80 <<< "$PLAIN_TEXT")"
cat <<< "$FORMATTED" > "$PROJECT_ROOT/target/debian-extended-description.txt"

# Create the license file for binary distributions (LICENSE + NOTICE)
cat LICENSE NOTICE > "$PROJECT_ROOT/target/debian-license.txt"

#
# Build the deb
#
#   --target
#     tells the builder everything it needs to know about where
#     the deb can run, including the architecture
#
#   --no-build
#     because this stop should follow a build
cargo deb --target "$TARGET" --deb-version "$PACKAGE_VERSION" --no-build

# Rename the resulting .deb file to use - instead of _ since this
# is consistent with our package naming scheme.
for file in target/"${TARGET}"/debian/*.deb; do
  base=$(basename "${file}")
  tail=${base#vector_${PACKAGE_VERSION}_}
  mv "${file}" target/"${TARGET}"/debian/vector-"${tail}";
done

#
# Move the deb into the artifacts dir
#

mv -v "target/$TARGET/debian"/*.deb target/artifacts
