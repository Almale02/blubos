#!/usr/bin/env bash
set -euo pipefail

# Always run from script's directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

IMG="blubos.img"
CODE_DEST="OVMF_CODE.fd"
VARS_DEST="OVMF_VARS.fd"

# Possible system install paths for OVMF
POSSIBLE_OVMF=(
  "/usr/share/OVMF/OVMF_CODE.fd"
  "/usr/share/OVMF/OVMF_CODE.4m.fd"
  "/usr/share/OVMF/x64/OVMF_CODE.fd"
  "/usr/share/OVMF/x64/OVMF_CODE.4m.fd"
  "/usr/share/edk2/ovmf/OVMF_CODE.fd"
  "/usr/share/edk2/ovmf/OVMF_CODE.4m.fd"
  "/usr/share/edk2/x64/OVMF_CODE.fd"
  "/usr/share/edk2/x64/OVMF_CODE.4m.fd"
  "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd"
  "/usr/share/edk2-ovmf/x64/OVMF_CODE.4m.fd"
  "/usr/share/qemu/OVMF_CODE.fd"
  "/usr/share/qemu/OVMF_CODE.4m.fd"
)

# OVMF_CODE=""
for candidate in "${POSSIBLE_OVMF[@]}"; do
    if [[ -f "$candidate" ]]; then
        OVMF_CODE="$candidate"
        break
    fi
done

if [[ -z "$OVMF_CODE" ]]; then
    echo "❌ Could not find any OVMF_CODE.fd on your system!"
    echo "   Try:  sudo apt install ovmf      (Debian/Ubuntu)"
    echo "         sudo dnf install edk2-ovmf (Fedora/RHEL)"
    echo "         sudo pacman -S edk2-ovmf   (Arch)"
    exit 1
fi

echo "✅ Found OVMF_CODE at $OVMF_CODE"
cp -f "$OVMF_CODE" "$CODE_DEST"

# Work out the matching VARS path depending on 2M vs 4M
if [[ "$OVMF_CODE" == *".4m.fd" ]]; then
    EXPECTED_VARS="${OVMF_CODE/OVMF_CODE.4m/OVMF_VARS.4m}"
else
    EXPECTED_VARS="${OVMF_CODE/OVMF_CODE/OVMF_VARS}"
fi

# If local VARS file missing, copy from system template
if [[ ! -f "$VARS_DEST" ]]; then
    if [[ -f "$EXPECTED_VARS" ]]; then
        cp "$EXPECTED_VARS" "$VARS_DEST"
        echo "✅ Copied VARS template from $EXPECTED_VARS"
    else
        echo "⚠️  Could not find VARS template, creating blank 4M file"
        qemu-img create -f raw "$VARS_DEST" 4M
    fi
fi

 # ✅ Launch QEMU
 # exec qemu-system-x86_64 -serial stdio \
 #     -drive if=pflash,format=raw,readonly=on,file="$CODE_DEST" \
 #     -drive if=pflash,format=raw,file="$VARS_DEST" \
 #     -drive format=raw,file="$IMG" \
 #     -trace "usb_ehci_*" -D qemu.log  \
 #     -device usb-ehci,id=ehci0, companion=none \
 # 	  -drive id=stick,file=blublang.img,if=none,format=raw \
 # 	  -device usb-storage,bus=ehci0.0,drive=stick,port=1
 #   #-device usb-tablet,bus=ehci0.0,port=2 \
 #    #-no-reboot -no-shutdown


qemu-system-x86_64 \
  -machine q35,usb=off \
  -serial stdio \
  -drive if=pflash,format=raw,readonly=on,file="$CODE_DEST" \
  -drive if=pflash,format=raw,file="$VARS_DEST" \
  -drive if=none,id=bootdisk,format=raw,file="$IMG" \
  -device virtio-blk-pci,drive=bootdisk,bootindex=0 \
  -device qemu-xhci,id=xhci \
  -device usb-kbd,bus=xhci.0 \
  -drive if=none,id=stick,file=blubos.img,format=raw \
  -trace events=trace-events.txt \
  -trace enable=usb_xhci_* \
  -D qemu.log


# /home/danad/dev/qemu/build/qemu-system-x86_64 \

  # -device usb-kbd,bus=ehci0.0,port=2 \
  # -device usb-storage,bus=ehci0.0,drive=stick,port=1 \
  # -trace "usb_ehci_*" -D qemu.log

     
# exec qemu-system-x86_64 \
#   -machine q35 \
# 	-serial stdio \
#   -device usb-ehci,id=ehci0 \
#   -drive id=stick,file=blubos.img,if=none,format=raw \
#   -device usb-storage,bus=ehci0.0,drive=stick,port=1 \
#   -drive if=pflash,format=raw,readonly=on,file="$CODE_DEST" \
#   -drive if=pflash,format=raw,file="$VARS_DEST" \
#   -drive format=raw,file="$IMG" \
#   -trace "usb_ehci_*" -D qemu.log
