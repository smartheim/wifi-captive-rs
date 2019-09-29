# Container focused operating system for OHX

[![Build Status](https://github.com/openhab-nodes/ohx-os/workflows/test/badge.svg)](https://github.com/openhab-nodes/ohx-os/actions)
[![](https://img.shields.io/badge/license-MIT-blue.svg)](http://opensource.org/licenses/MIT)

This repository hosts scripts to assemble and deploy operating system images
with OHX preinstalled. The images are deployed to the Github releases page of this repo.
This repo also contains 

The log-in user is called "ohx" with a default password "ohx", ssh is enabled.

Supported systems are:
* Any UEFI equiped x86-64 system like the Intel NUC
* The Raspberry PI 3 and 4.
* The Pine64

You can use a flashed SD-Card with 
Please wait about 4 minutes on the very first boot, because the sd-card 

## About the operating system choice

The operating system is based on openSUSE Kubic (which is a variant of openSUSE MicroOS) with customized [Ignition](https://en.opensuse.org/Kubic:MicroOS/Ignition)
and cloud-init first-boot scripts or on BalenaOS. This is not yet decided.

BalenaOS is more mature, but does have a strong bound to the balena cloud and an own supervisior. It also use the original docker daemon, instead of a rootless software container alternative like it is done with Redhats Fedora IoT and openSUSEs Kubic. Fedora IoT as well as openSUSE Kubic both have a boot time of about 5 minutes and only support a very limited selection of single board ARM systems (fedora: RPI3, openSUSE: RPI3, Pine64).

A non-goal for OHX-OS is a custom build OS, based on [buildroot](https://buildroot.org/).
This would require a custom update mechanism, CVE tracker and more and is not the focus of the OHX project.

## Assemble scripts

Prerequirements:

* skopeo
* A dockerhub credentials file (`docker_credentials.inc`) with a credentials line following the pattern "DOCKER_CRED=username:password".

The `./build_microos.sh` and `./build_balena.sh` scripts first downloads the current openSUSE Kubic images or balena OS images respectively for all supported systems (x86-64, aarch64-rpi3). They decompress and mount the images.

The Kubic script will write the *Ignition* and cloud-init first-boot instructions.

The balena script will write the `config.json` file.

In a final step in both script types the the wifi-connect binary and aux files as well as the OHX core containers are added and finally all images are compressed again and temporary directories are removed.

## Deployment

Prerequirements:

* skopeo
* A github credentials file (`github_access_token.inc`) with a credentials line following the pattern "GITHUB_CRED=username:access_token". Create an access token in the OHX organisation page.

Execute the `deploy.sh` script to:

* Create a new release and add a message with the current date to it.
* Attach / Upload the generated images
