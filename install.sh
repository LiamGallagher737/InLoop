#!/bin/sh
# Install or update the `inloop` binary from a GitHub release.
#
# The repo is private, so this uses the GitHub CLI (`gh`), signed in with an
# account that can read it. One line, from anywhere:
#
#   gh api repos/LiamGallagher737/InLoop/contents/install.sh -H "Accept: application/vnd.github.raw" | sh
#
# Settings, all optional:
#   INLOOP_VERSION      a release tag such as v0.1.0 (default: the latest release)
#   INLOOP_INSTALL_DIR  where the binary goes (default: ~/.local/bin)
#   INLOOP_REPO         owner/name to download from (default: LiamGallagher737/InLoop)
#
# macOS and Linux only. On Windows, download the .zip from the release page.

set -eu

repo="${INLOOP_REPO:-LiamGallagher737/InLoop}"
dest="${INLOOP_INSTALL_DIR:-$HOME/.local/bin}"
version="${INLOOP_VERSION:-}"

say() { printf '%s\n' "$*"; }
fail() { printf 'inloop install: %s\n' "$*" >&2; exit 1; }

command -v gh >/dev/null 2>&1 ||
	fail "needs the GitHub CLI. Install it from https://cli.github.com, run 'gh auth login', then try again."
gh auth status >/dev/null 2>&1 ||
	fail "the GitHub CLI isn't signed in. Run 'gh auth login', then try again."

case "$(uname -s)" in
Darwin) os=apple-darwin ;;
Linux) os=unknown-linux-gnu ;;
*) fail "unsupported system $(uname -s). On Windows, download the .zip from the release page." ;;
esac
case "$(uname -m)" in
arm64 | aarch64) arch=aarch64 ;;
x86_64 | amd64) arch=x86_64 ;;
*) fail "unsupported processor $(uname -m)" ;;
esac
target="$arch-$os"

if [ -z "$version" ]; then
	version="$(gh release view --repo "$repo" --json tagName --jq .tagName 2>/dev/null)" ||
		fail "couldn't read the latest release of $repo. Does your GitHub account have access to it?"
fi
name="inloop-$version-$target"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading $name from $repo"
gh release download "$version" --repo "$repo" --dir "$tmp" \
	--pattern "$name.tar.gz" --pattern SHA256SUMS ||
	fail "couldn't download $name.tar.gz from release $version"

# Check the archive against the release's checksums.
expected="$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS has no entry for $name.tar.gz"
if command -v sha256sum >/dev/null 2>&1; then
	actual="$(sha256sum "$tmp/$name.tar.gz" | awk '{ print $1 }')"
else
	actual="$(shasum -a 256 "$tmp/$name.tar.gz" | awk '{ print $1 }')"
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $name.tar.gz; not installing"

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dest"
# Install via a temporary name and rename, so a running inloop is never half-written.
cp "$tmp/$name/inloop" "$dest/.inloop.new"
chmod 755 "$dest/.inloop.new"
if [ "$os" = apple-darwin ]; then
	# The binary isn't notarised; make sure Gatekeeper doesn't block it.
	xattr -d com.apple.quarantine "$dest/.inloop.new" 2>/dev/null || true
fi
mv -f "$dest/.inloop.new" "$dest/inloop"

say "Installed $("$dest/inloop" --version) to $dest/inloop"

case ":$PATH:" in
*":$dest:"*) ;;
*)
	say ""
	say "$dest is not on your PATH. Add this line to your shell profile (~/.zshrc or ~/.bashrc):"
	say "  export PATH=\"$dest:\$PATH\""
	say "Claude Code runs InLoop's hooks and MCP server by name, so they need it on the PATH too."
	;;
esac

say ""
say "Next, in a repo: 'inloop init'. It sets up a new project, and in one that already uses InLoop it adds the git hook."
