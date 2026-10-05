#!/bin/sh
# Runs on the host: give the VM that ../gxwi/dev/boot.sh started a next
# release to upgrade to. It packages the stand-in 2026.9-1 edition in
# dev/next-release, publishes it with the dev.peios.net its floor asks for
# (from ../pkgs' pool) in a small repository on the share, signed with the
# development key, and adds it in the guest.
#
# The upgrade can be made once a boot: each boot starts from the image's
# 2026.8 again.
#
#     dev/repo.sh          build it if it isn't there, and add it
#     dev/repo.sh --new    build it again
set -eu
cd "$(dirname "$0")/.."
share=../gxwi/target/vmshare
repo=$share/releaserepo
pool=../pkgs/_repo2_/p
key=../pkgs/dev-signing.key
[ -d "$share" ] || { echo "no $share: boot the VM from ../gxwi first" >&2; exit 1; }
[ "${1:-}" = --new ] && rm -rf "$repo"
if [ ! -d "$repo" ]; then
    tool=$(mktemp -d)
    trap 'rm -rf "$tool"' EXIT
    (cd ../peipkg && go build -o "$tool/peipkg-repo" ./cmd/peipkg-repo)
    # The development keyring signs the package with the key the repository
    # is signed with.
    keyring=$(cd ../pkgs && pwd)/dev.keyring.pekit.toml
    (cd dev/next-release && pekit package --no-gates --keyring "$keyring")
    edition=$(ls -t dev/next-release/out/package/*/dev.peios.peios-experimental_*.peipkg | head -1)
    "$tool/peipkg-repo" init "$repo" --name peios-dev-release --key "$key" \
        --description "A stand-in next release for trying Upgrade Peios" > /dev/null
    "$tool/peipkg-repo" publish "$repo" "$edition" "$pool"/dev.peios.net/0.1.7-1/*.peipkg --key "$key"
fi
anchor=$(sed -n 's/.*"fingerprint": "\([0-9a-f]*\)".*/\1/p' "$repo/repo.json" | head -1)
# Resolving needs every configured repository trusted, the medium's too,
# whose trust ceremony a fresh boot may not have run yet.
../gxwi/dev/guest.sh "peipkg repo add peios-dev-release file:///share/releaserepo --anchor $anchor
peipkg repo add peios-medium; peipkg repo list"
