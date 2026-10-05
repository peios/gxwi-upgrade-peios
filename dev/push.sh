#!/bin/sh
# Runs on the host: build Upgrade Peios and put it in the share of the VM
# that ../gxwi/dev/boot.sh started, with its icon for the base theme and its
# declaration for the catalogue. GXWI's dev service puts them where a package
# would, and its dev loop restarts on a new one.
#
# The upgrade-peios it drives goes on the share too, built on its own from
# ../peiosutils, until a peiosutils with the driven mode is in the image. It
# drives the peipkg ../gxwi-package-manager/dev/push.sh puts there.
#
# The rename makes the new binary appear whole, never half-written.
set -eu
cd "$(dirname "$0")/.."
. dev/env.sh
[ -d ../gxwi/target/vmshare ] || { echo "no ../gxwi/target/vmshare: boot the VM from ../gxwi first" >&2; exit 1; }
share=$(cd ../gxwi/target/vmshare && pwd)
cargo build --release
(cd ../peiosutils && cargo build --release -p pu_upgrade_peios --bin upgrade-peios)
cp ../peiosutils/target/release/upgrade-peios "$share/upgrade-peios.new"
mv "$share/upgrade-peios.new" "$share/upgrade-peios"
mkdir -p "$share/icons/base"
cp gxwi-upgrade-peios.svg "$share/icons/base/dev.peios.gxwi-upgrade-peios.svg"
mkdir -p "$share/apps"
cp dev.peios.gxwi-upgrade-peios.toml "$share/apps/dev.peios.gxwi-upgrade-peios.toml"
cp target/release/gxwi-upgrade-peios "$share/gxwi-upgrade-peios.new"
mv "$share/gxwi-upgrade-peios.new" "$share/gxwi-upgrade-peios"
