Synthetic 440 Hz tone, 0.2 seconds, generated locally with FFmpeg; no recorded or copyrighted music.
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 0.2 -ac 1 -metadata title="Local fixture" -metadata artist="Private Music" -metadata album="Local tests" sample.<extension>
These tiny fixtures exercise actual MP3, M4A, AAC, WAV and FLAC imports and decoding.
