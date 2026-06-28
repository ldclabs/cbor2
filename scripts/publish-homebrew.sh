#!/bin/sh
# Generate and publish the Homebrew formula for cbor2-cli.
#
# Required in CI:
#   HOMEBREW_TAP_TOKEN  GitHub token with write access to HOMEBREW_TAP_REPO
#
# Optional:
#   HOMEBREW_TAP_REPO   Defaults to ldclabs/homebrew-tap
#   HOMEBREW_TAP_BRANCH Defaults to main
#   HOMEBREW_TAP_DIR    Existing local tap checkout to update instead of cloning
#                       Defaults to /opt/homebrew/Library/Taps/ldclabs/homebrew-tap
#                       when that checkout exists
#   LOCAL_HOMEBREW_TAP_DIR
#                       Override the default local tap checkout path
#   CHECKSUM_DIR         Directory containing release .sha256 files
#   DRY_RUN=1           Print the generated formula and skip git operations

set -eu

REPO="${REPO:-ldclabs/cbor2}"
FORMULA_NAME="${FORMULA_NAME:-cbor2-cli}"
FORMULA_CLASS="${FORMULA_CLASS:-Cbor2Cli}"
BINARY_NAME="${BINARY_NAME:-cbor}"
TAP_REPO="${HOMEBREW_TAP_REPO:-ldclabs/homebrew-tap}"
TAP_BRANCH="${HOMEBREW_TAP_BRANCH:-main}"
FORMULA_PATH="${HOMEBREW_FORMULA_PATH:-Formula/${FORMULA_NAME}.rb}"
LOCAL_TAP_DIR="${LOCAL_HOMEBREW_TAP_DIR:-/opt/homebrew/Library/Taps/ldclabs/homebrew-tap}"
TAG="${1:-${GITHUB_REF_NAME:-}}"

info() { printf '%s\n' "$1" >&2; }
error() { printf 'Error: %s\n' "$1" >&2; exit 1; }

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || error "Missing required command: $1"
}

normalize_tag() {
    RAW_TAG="$1"
    RAW_TAG="${RAW_TAG#refs/tags/}"

    if [ -z "$RAW_TAG" ]; then
        error "Usage: $0 <tag>. Example: $0 v1.0.2"
    fi

    case "$RAW_TAG" in
        v*) printf '%s\n' "$RAW_TAG" ;;
        *) printf 'v%s\n' "$RAW_TAG" ;;
    esac
}

fetch_checksum() {
    ASSET_NAME="$1"

    if [ -n "${CHECKSUM_DIR:-}" ] && [ -f "${CHECKSUM_DIR}/${ASSET_NAME}.sha256" ]; then
        HASH=$(awk '{print $1}' "${CHECKSUM_DIR}/${ASSET_NAME}.sha256" | tr -d '\r\n')
    else
        CHECKSUM_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}.sha256"
        HASH=$(curl -fsSL "$CHECKSUM_URL" | awk '{print $1}' | tr -d '\r\n')
    fi

    if [ -z "$HASH" ]; then
        error "Could not read checksum for ${ASSET_NAME}"
    fi

    printf '%s\n' "$HASH"
}

write_formula() {
    cat <<EOF
class ${FORMULA_CLASS} < Formula
  desc "CBOR command-line converter and diagnostic notation inspector"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "${BASE_URL}/${BINARY_NAME}-macos-arm64", using: :nounzip
      sha256 "${MACOS_ARM64_SHA}"
    else
      url "${BASE_URL}/${BINARY_NAME}-macos-x86_64", using: :nounzip
      sha256 "${MACOS_X86_64_SHA}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "${BASE_URL}/${BINARY_NAME}-linux-arm64", using: :nounzip
      sha256 "${LINUX_ARM64_SHA}"
    else
      url "${BASE_URL}/${BINARY_NAME}-linux-x86_64", using: :nounzip
      sha256 "${LINUX_X86_64_SHA}"
    end
  end

  def install
    binary = Dir["${BINARY_NAME}-*"].first
    chmod 0755, binary
    bin.install binary => "${BINARY_NAME}"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/${BINARY_NAME} --version")
    assert_match "{1: 2}", shell_output("#{bin}/${BINARY_NAME} a10102")
  end
end
EOF
}

need_cmd awk
need_cmd git

if [ -z "${CHECKSUM_DIR:-}" ]; then
    need_cmd curl
fi

TAG=$(normalize_tag "$TAG")
VERSION="${TAG#v}"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

info "Generating Homebrew formula for ${REPO} ${TAG}..."

MACOS_ARM64_SHA=$(fetch_checksum "${BINARY_NAME}-macos-arm64")
MACOS_X86_64_SHA=$(fetch_checksum "${BINARY_NAME}-macos-x86_64")
LINUX_ARM64_SHA=$(fetch_checksum "${BINARY_NAME}-linux-arm64")
LINUX_X86_64_SHA=$(fetch_checksum "${BINARY_NAME}-linux-x86_64")

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

FORMULA_TMP="${TMPDIR}/${FORMULA_NAME}.rb"
write_formula > "$FORMULA_TMP"

if [ "${DRY_RUN:-}" = "1" ]; then
    cat "$FORMULA_TMP"
    exit 0
fi

if [ -n "${HOMEBREW_TAP_DIR:-}" ]; then
    TAP_DIR="$HOMEBREW_TAP_DIR"
    [ -d "${TAP_DIR}/.git" ] || error "HOMEBREW_TAP_DIR is not a git checkout: ${TAP_DIR}"
elif [ -d "${LOCAL_TAP_DIR}/.git" ]; then
    TAP_DIR="$LOCAL_TAP_DIR"
else
    [ -n "${HOMEBREW_TAP_TOKEN:-}" ] || error "Set HOMEBREW_TAP_TOKEN or HOMEBREW_TAP_DIR to publish the formula"
    TAP_DIR="${TMPDIR}/tap"
    TAP_ASKPASS="${TMPDIR}/git-askpass.sh"
    cat > "$TAP_ASKPASS" <<'EOF'
#!/bin/sh
case "$1" in
    *Username*) printf '%s\n' "x-access-token" ;;
    *Password*) printf '%s\n' "${HOMEBREW_TAP_TOKEN:?}" ;;
    *) printf '\n' ;;
esac
EOF
    chmod 700 "$TAP_ASKPASS"
    GIT_ASKPASS="$TAP_ASKPASS" GIT_TERMINAL_PROMPT=0 \
        git clone --branch "$TAP_BRANCH" "https://github.com/${TAP_REPO}.git" "$TAP_DIR"
fi

mkdir -p "${TAP_DIR}/$(dirname "$FORMULA_PATH")"
cp "$FORMULA_TMP" "${TAP_DIR}/${FORMULA_PATH}"

git -C "$TAP_DIR" config user.name "${GIT_COMMITTER_NAME:-github-actions[bot]}"
git -C "$TAP_DIR" config user.email "${GIT_COMMITTER_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}"

if [ -z "$(git -C "$TAP_DIR" status --porcelain -- "$FORMULA_PATH")" ]; then
    info "Homebrew formula is already up to date."
    exit 0
fi

git -C "$TAP_DIR" add "$FORMULA_PATH"
git -C "$TAP_DIR" commit -m "Update ${FORMULA_NAME} formula to ${TAG}"

if [ "${PUSH:-1}" = "1" ]; then
    if [ -n "${TAP_ASKPASS:-}" ]; then
        GIT_ASKPASS="$TAP_ASKPASS" GIT_TERMINAL_PROMPT=0 \
            git -C "$TAP_DIR" push origin "HEAD:${TAP_BRANCH}"
    else
        git -C "$TAP_DIR" push origin "HEAD:${TAP_BRANCH}"
    fi
    info "Published ${FORMULA_PATH} to ${TAP_REPO} ${TAP_BRANCH}."
else
    info "Updated ${FORMULA_PATH} in ${TAP_DIR}; PUSH=0 so changes were not pushed."
fi
