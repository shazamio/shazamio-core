#!/usr/bin/env bash

set -e

apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install \
    clang \
    autoconf \
    automake \
    build-essential \
    cmake \
    libasound2-dev \
    libass-dev \
    libavutil56 libavutil-dev \
    libavformat58 libavformat-dev \
    libavfilter7 libavfilter-dev \
    libavdevice58 libavdevice-dev \
    libfreetype6-dev \
    libgnutls28-dev \
    libmp3lame-dev \
    libsdl2-dev \
    libtool \
    libva-dev \
    libvdpau-dev \
    libvorbis-dev \
    libxcb1-dev \
    libxcb-shm0-dev \
    libxcb-xfixes0-dev \
    pkg-config \
    wget \
    yasm \
    zlib1g-dev \
    openssl \
    libsasl2-dev \
    libsasl2-2 \
    python3-dev \
    python3-pip \
    curl

curl -o rustup.sh --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs
sh rustup.sh -y
source $HOME/.cargo/env
rustup update
rustc -V

cargo install cargo-chef --locked

/usr/bin/python3 -m pip install --upgrade pip
/usr/bin/python3 -m pip install --upgrade maturin==1.4.0