#!/bin/fish
set offset (math "2048*512")
sudo mount -o loop,offset=$offset blublang.img mnt
