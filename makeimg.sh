#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
IMG_DIR="$SCRIPT_DIR/img"

IMG="$IMG_DIR/blublang.img"
MNT="$IMG_DIR/mnt"
SIZE=64M

rm -f "$IMG"
mkdir -p "$MNT"

# 1. Create empty image
truncate -s "$SIZE" "$IMG"

# 2. Partition with GPT + ESP
parted -s "$IMG" mklabel gpt
parted -s "$IMG" mkpart ESP fat32 2048s 100%
parted -s "$IMG" set 1 boot on

# 3. Attach loop
LOOP=$(sudo losetup -Pf --show "$IMG")

# 4. Format ESP
sudo mkfs.fat -F32 "${LOOP}p1"

# 5. Mount and copy all files from project /boot
sudo mount -o uid=$(id -u),gid=$(id -g) "${LOOP}p1" "$MNT"
# Copy everything inside script_dir/boot recursively
sudo cp -rT "$SCRIPT_DIR/boot" "$MNT"

sudo umount "$MNT"
sudo losetup -d "$LOOP"

echo "✅ $IMG ready."
