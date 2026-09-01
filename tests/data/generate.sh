#!/usr/bin/env bash
# Regenerates the four probe files the golden fingerprint tests read.
#
# They are synthetic: two linear chirps plus four fixed tones, so they carry no
# third-party licence and no attribution. Real music would carry both, and no
# recording short enough to commit is free of either.
#
# The content is chosen to survive lossy encoding: the chirps sweep across the
# bands the fingerprint peaks in, and the fixed tones give every band a peak that
# stays put. A plain sine wave yields a near-empty signature.
#
# Re-running this reproduces `probe.mp3`, `probe.flac` and `probe.m4a` byte for
# byte. It does not reproduce `probe.ogg`: an Ogg stream carries a random serial
# number, so 80 of its 48330 bytes change per run. The decoded audio does not, and neither does
# the fingerprint -- so `probe.flac.uri`, the one golden left, survives a
# regeneration. Checked on ffmpeg 8.0.1; another build may re-encode differently,
# and then the golden has to be rewritten alongside the audio.
set -euo pipefail

cd "$(dirname "$0")"

ffmpeg -y -f lavfi -i "aevalsrc=\
0.30*sin(2*PI*(300+180*t)*t)+0.22*sin(2*PI*1237*t)+0.16*sin(2*PI*3001*t)|\
0.28*sin(2*PI*(450+240*t)*t)+0.20*sin(2*PI*1601*t)+0.14*sin(2*PI*2699*t)\
:s=44100:d=8" -c:a pcm_s16le probe.wav

ffmpeg -y -i probe.wav -c:a libmp3lame -b:a 128k probe.mp3
ffmpeg -y -i probe.wav -c:a libvorbis -b:a 96k probe.ogg
ffmpeg -y -i probe.wav -c:a flac -compression_level 8 probe.flac
ffmpeg -y -i probe.wav -c:a aac -b:a 128k probe.m4a

rm probe.wav
