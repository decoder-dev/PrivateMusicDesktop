#!/usr/bin/env python3
"""Random HLS playlists, weighted toward malformed and unusual input."""
import random, sys, pathlib
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
n = int(sys.argv[2]); random.seed(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
NL = ["\n", "\r\n", "\r", "\n", "\n", " ", "\u0085", "\n\n"]
def ws(): return random.choice(["", "", "", " ", "  ", "\t", " "])
def word(): return "".join(random.choice("abcdefghijklmnopqrstuvwxyz0123456789_-") for _ in range(random.randint(1, 8)))
def uri():
    r = random.random()
    if r < 0.15: return f"https://cdn.example/{word()}/{word()}.ts"
    path = "/".join(word() for _ in range(random.randint(1, 3)))
    ext = random.choice([".ts", ".m4s", ".aac", ".mp3", ".m3u8", "", ".mp4"])
    q = random.choice(["", "", "", "?q=1&r=2"])
    return path + ext + q
def q(s): return random.choice(['"%s"' % s, '"%s"' % s, '"%s"' % s, s, "'%s'" % s]) if random.random() < 0.97 else s
def attrs(items):
    items = items[:]
    random.shuffle(items)
    sep = random.choice([",", ",", ", ", " ,"])
    return sep.join(items)
def rng():
    return random.choice(["100@200", "512", "0@0", "@5", "x", "100@x", "-5@3", "100@-1", "", "7@", "1@2@3", "99999999999999999999@1", "+8@+9", "  5@6  "])
def hexiv():
    kind = random.random()
    if kind < 0.5: return "0x" + "".join(random.choice("0123456789abcdefABCDEF") for _ in range(32))
    if kind < 0.6: return "0X" + "".join(random.choice("0123456789abcdef") for _ in range(31))
    if kind < 0.7: return "".join(random.choice("0123456789abcdef") for _ in range(32))
    if kind < 0.8: return "0x" + "".join(random.choice("0123456789abcdefg+z") for _ in range(random.randint(0, 40)))
    return "0x" + "".join(random.choice("0123456789abcdef") for _ in range(random.choice([30, 34, 2, 0])))
def key_line():
    m = random.choice(["AES-128", "AES-128", "aes-128", "NONE", "none", "SAMPLE-AES", "AES-256", None])
    a = []
    if m is not None: a.append("METHOD=" + m)
    if random.random() < 0.9: a.append("URI=" + q(uri()))
    if random.random() < 0.5: a.append("IV=" + hexiv())
    if random.random() < 0.2: a.append("KEYFORMAT=\"identity\"")
    return "#EXT-X-KEY:" + attrs(a)
def map_line():
    a = []
    if random.random() < 0.93: a.append("URI=" + q(uri()))
    if random.random() < 0.5: a.append("BYTERANGE=" + q(rng()))
    return "#EXT-X-MAP:" + attrs(a)
def media():
    L = ["#EXTM3U"] if random.random() < 0.97 else []
    L.append(random.choice(["#EXT-X-VERSION:3", "#EXT-X-TARGETDURATION:10", "#EXT-X-VERSION:7"]))
    if random.random() < 0.8: L.append(random.choice(["#EXT-X-MEDIA-SEQUENCE:", "#EXT-X-MEDIA-SEQUENCE:", "#EXT-X-MEDIA-SEQUENCE: "]) + random.choice(["0", "5", "123456", "x", "-3", "+4", "99999999999999999999", "", "7 "]))
    count = random.choice([0, 1, 2, 3, 5, 8, 20])
    for _ in range(count):
        r = random.random()
        if r < 0.15: L.append(key_line())
        elif r < 0.25: L.append(map_line())
        elif r < 0.33: L.append("#EXT-X-DISCONTINUITY" + random.choice(["", "", "-SEQUENCE:4"]))
        elif r < 0.45: L.append("#EXT-X-BYTERANGE:" + rng())
        elif r < 0.50: L.append("# comment " + word())
        elif r < 0.53: L.append("")
        if random.random() < 0.8: L.append("#EXTINF:%.3f," % random.uniform(1, 10))
        L.append(random.choice(["", "", " ", "\t"]) + uri() + random.choice(["", "", " "]))
    if random.random() < 0.5: L.append("#EXT-X-ENDLIST")
    return L
def master():
    L = ["#EXTM3U"]
    groups = [word() for _ in range(random.randint(1, 3))]
    for _ in range(random.randint(0, 4)):
        a = [random.choice(["TYPE=AUDIO", "TYPE=AUDIO", "TYPE=audio", "TYPE=VIDEO", "TYPE=SUBTITLES"])]
        if random.random() < 0.9: a.append("GROUP-ID=" + q(random.choice(groups)))
        if random.random() < 0.9: a.append("URI=" + q(uri()))
        if random.random() < 0.7: a.append("NAME=" + q(word() + ", " + word()))
        if random.random() < 0.3: a.append(random.choice(["DEFAULT=YES", "DEFAULT=NO", "DEFAULT=yes"]))
        if random.random() < 0.3: a.append(random.choice(["AUTOSELECT=YES", "AUTOSELECT=NO"]))
        L.append("#EXT-X-MEDIA:" + attrs(a))
    for _ in range(random.randint(0, 5)):
        a = []
        if random.random() < 0.9: a.append("BANDWIDTH=" + random.choice(["64000", "128000", "128000", "256000", "x", "99999999999999999999", "-1"]))
        if random.random() < 0.9:
            codecs = random.sample(["mp4a.40.2", "mp4a.40.5", "avc1.4d401f", "hvc1.1", "ac-3", "ec-3", "mp3", "aac", "vp9", "av01.0", "opus", " mp4a.40.2"], random.randint(0, 3))
            a.append("CODECS=\"" + ", ".join(codecs) + "\"")
        if random.random() < 0.4: a.append("AUDIO=" + q(random.choice(groups)))
        if random.random() < 0.3: a.append("RESOLUTION=640x360")
        L.append(random.choice(["#EXT-X-STREAM-INF:", "#EXT-X-STREAM-INF:", "#EXT-X-STREAM-INF"]) + attrs(a))
        if random.random() < 0.9:
            L.append(uri())
    return L
for i in range(n):
    L = master() if random.random() < 0.45 else media()
    text = ""
    for line in L:
        text += ws() + line + ws() + random.choice(NL)
    (out / f"p{i:05d}.m3u8").write_text(text, encoding="utf-8", newline="")
# segment limit
(out / "pbig.m3u8").write_text("#EXTM3U\n" + "".join(f"s{i}.ts\n" for i in range(4001)))
(out / "pbig_ok.m3u8").write_text("#EXTM3U\n" + "".join(f"s{i}.ts\n" for i in range(4000)))
