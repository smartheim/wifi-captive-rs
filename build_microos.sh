#!/bin/sh -e
#echo "Password hash for password 'ohx'"
#echo "ohx" | mkpasswd --method=SHA-512 --stdin

TARGET=/dev/mmcblk0
BASEIMG=openSUSE-MicroOS.aarch64-RaspberryPi.raw
BASEIMGTYPE=devel_kubic_arm
ARCH=arm

source ./docker_credentials.inc

if [ ! -f "$BASEIMG" ]; then
wget https://download.opensuse.org/repositories/devel:/kubic:/images/$BASEIMGTYPE/$BASEIMG.xz
xz --decompress $BASEIMG.xz
fi


# Download docker images
echo "Download images for $ARCH"
if [ ! -d "image_root_$ARCH" ]; then
mkdir image_root_$ARCH
skopeo copy "--screds=$DOCKER_CRED" docker://docker.io/openhabx/addon123-$ARCH dir:image_root_$ARCH || (rm -rf image_root_$ARCH && false)
fi

OFFSET=$(expr $(fdisk -l "$BASEIMG" -o start | tail -n 1) \* 512)

# Copy ignition file - sets up systemd unit files
echo "Patching ignition at $OFFSET,subvol=@/boot/writable"
rm -rf boot_mount||true
mkdir boot_mount
sudo mount -o loop,offset=$OFFSET,subvol=@/boot/writable $BASEIMG boot_mount
sudo cp ignition.firstboot boot_mount/
sudo umount ./boot_mount
rm -rf boot_mount||true

# Copy ignition file - sets up systemd unit files
echo "Patching cloud-init at $OFFSET,subvol=@/var"
rm -rf var_mount||true
mkdir var_mount
sudo mount -o loop,offset=$OFFSET,subvol=@/var $BASEIMG var_mount
sudo cp user-data var_mount/lib/cloud/
sudo umount ./var_mount
rm -rf var_mount||true

# Copy image files
echo "Copying images to $OFFSET,subvol=@/srv"
rm -rf srv_mount||true
mkdir srv_mount
sudo mount -o loop,offset=$OFFSET,subvol=@/srv $BASEIMG srv_mount
sudo rm -rf srv_mount/ohx_bootstrap||true
sudo mkdir srv_mount/ohx_bootstrap
sudo cp -r image_root_$ARCH/* srv_mount/ohx_bootstrap
sudo umount srv_mount
rm -rf srv_mount||true

IMAGESIZE=$(ls -lh $BASEIMG|awk '{print $5}')
echo "Copy $IMAGESIZE to mmc"
sudo dd if=$BASEIMG of=$TARGET bs=1M status=progress
