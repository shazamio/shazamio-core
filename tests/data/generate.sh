#!/usr/bin/env bash
# Regenerates the probe files the golden fingerprint tests read.
#
# They are synthetic: chirps, fixed tones and a harmonic chord, so they carry no
#  third-party licence and no attribution. Real music would carry both, and no
#  recording short enough to commit is free of either.
#
# `probe.*` is the format matrix: one source encoded five ways, so every decoder
#  path runs against identical audio. Its content is chosen to survive lossy
#  encoding: the chirps sweep across the bands the fingerprint peaks in, and the
#  fixed tones give every band a peak that stays put. A plain sine wave yields a
#  near-empty signature.
#
# `chord.flac` is what a change to decoding or resampling is judged on, because
#  `probe.*` is the wrong signal for that. Its two channels carry different chirps,
#  so a change to the downmix moves far more than it would on a stereo mix, and it
#  holds nothing above 3.2 kHz, so every peak in the 3500 to 5500 Hz band is an
#  artifact whichever pipeline produced it. The chord carries the same partials in
#  both channels, phase shifted, and reaches 6.6 kHz.
#
# Re-running this reproduces every file byte for byte except `probe.ogg` and
#  `probe.opus`: an Ogg stream carries a random serial number, so a handful of bytes
#  change per run. The decoded audio does not, and neither does the fingerprint, so
#  the goldens survive a regeneration. Checked on `ffmpeg` 8.0.1; another build may
#  re-encode differently, and then the goldens have to be rewritten alongside the
#  audio.
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
# `-vbr constrained` because the default unconstrained VBR ignores `-b:a` on this
#  signal and writes 185 kbps, four times the size of every other probe.
ffmpeg -y -i probe.wav -c:a libopus -b:a 96k -vbr constrained probe.opus

rm probe.wav

# One expression per channel of the chord. The right channel is phase shifted and
#  has every third partial pulled down, which is what makes the two correlated
#  without being identical.
chord_side() {
  awk -v phase="$1" -v tilt="$2" 'BEGIN {
    count = split("110 165 220 275 330 440 550 660 880 1100 1320 1760 2200 2640 3300 4400 5500 6600", partials, " ")
    for (number = 1; number <= count; number++) {
      amplitude = 0.9 / number ^ 0.6 * (1 - tilt * ((number - 1) % 3))
      printf "%s%.4f*sin(2*PI*%d*t+%s)", (number > 1 ? "+" : ""), amplitude, partials[number], phase
    }
  }'
}

# The chord is struck twice a second and decays, so the fingerprint has onsets to
#  lock onto instead of one continuous tone.
chord_envelope="(0.35+0.65*exp(-6*mod(t,0.5)))"

ffmpeg -y -f lavfi -i "aevalsrc=exprs='\
0.16*${chord_envelope}*($(chord_side 0 0))|\
0.16*${chord_envelope}*($(chord_side 0.35 0.08))\
':s=44100:d=8" -c:a pcm_s16le chord.wav

ffmpeg -y -i chord.wav -c:a flac -compression_level 8 chord.flac

rm chord.wav
