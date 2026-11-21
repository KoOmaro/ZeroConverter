# Test fixtures

Small binary files used by the Rust tests. They are never shipped with the app.

## `minimal.ogg`

A one-second 440 Hz sine tone, stereo, 44.1 kHz, Ogg Vorbis. It exercises the Symphonia Ogg/Vorbis decode path in the audio tests. Generated, so it carries no third-party content:

```sh
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 2 -c:a libvorbis -q:a 3 -map_metadata -1 -fflags +bitexact -flags:a +bitexact minimal.ogg
```
