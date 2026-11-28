#!/usr/bin/env python3
import argparse
import io
import json
from pathlib import Path
import re
import subprocess
import time
import wave
import xml.etree.ElementTree as ET


PACKAGE = "com.dec.privatemusic2.android"
ACTIVITY = "com.dec.privatemusic.compose.app.MainActivity"
DOWNLOADS = "files/native-engine/downloads"
TITLE = "Offline smoke track"


def adb(*args, data=None):
    return subprocess.run(
        ["adb", *args], input=data, stdout=subprocess.PIPE, check=True, timeout=25,
    ).stdout


def seed_download():
    audio = io.BytesIO()
    sample_rate = 48000
    duration = 90
    with wave.open(audio, "wb") as output:
        output.setparams((1, 2, sample_rate, 0, "NONE", "not compressed"))
        output.writeframes(bytes(sample_rate * 2 * duration))
    track = {
        "track_id": 1, "owner_id": 1, "title": TITLE, "artist": "Private Music",
        "duration": duration, "is_hq": False,
    }
    entry = {"track": track, "file": "smoke.wav", "bytes": len(audio.getvalue()), "added": 1}
    adb("shell", "run-as", PACKAGE, "mkdir", "-p", DOWNLOADS)
    adb("exec-in", f"run-as {PACKAGE} sh -c 'cat > {DOWNLOADS}/smoke.wav'", data=audio.getvalue())
    adb(
        "exec-in", f"run-as {PACKAGE} sh -c 'cat > {DOWNLOADS}/index.json'",
        data=json.dumps([entry]).encode(),
    )


def find_label(output, label):
    remote = "/sdcard/private-music-smoke.xml"
    for _ in range(5):
        try:
            adb("shell", "uiautomator", "dump", remote)
            content = adb("exec-out", "cat", remote)
            (output / "screen.xml").write_bytes(content)
            for node in ET.fromstring(content).iter("node"):
                labels = node.get("text", "").splitlines() + node.get("content-desc", "").splitlines()
                if node.get("package") == PACKAGE and label in labels:
                    return node
        except (subprocess.CalledProcessError, ET.ParseError):
            pass
        time.sleep(1)
    raise RuntimeError(f"Could not find {label!r} in the app UI")


def tap(node):
    left, top, right, bottom = map(int, re.findall(r"\d+", node.attrib["bounds"]))
    adb("shell", "input", "tap", str((left + right) // 2), str((top + bottom) // 2))


def check_playback_state(output, label, code, require_progress=False):
    for _ in range(12):
        state = adb("shell", "dumpsys", "media_session").decode()
        (output / "media-session.txt").write_text(state, encoding="utf-8")
        session = re.search(
            rf"(?m)^ +package={re.escape(PACKAGE)}\s*\n(.*?)(?=\n {{4}}\S|\Z)", state, re.DOTALL,
        )
        if session and TITLE in session[1]:
            playback = re.search(
                rf"PlaybackState\s*\{{state=(?:{label}\({code}\)|{code}), position=(\d+)", session[1],
            )
            if playback and (not require_progress or int(playback[1]) > 0):
                return
        time.sleep(1)
    raise RuntimeError(f"The seeded offline track did not reach {label}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("apk")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    try:
        adb("install", "-g", args.apk)
        seed_download()
        launch = adb("shell", "am", "start", "-W", "-n", f"{PACKAGE}/{ACTIVITY}").decode()
        (args.output / "launch.txt").write_text(launch, encoding="utf-8")
        if "Status: ok" not in launch:
            raise RuntimeError("Android did not launch Private Music")
        find_label(args.output, "Главная")
        (args.output / "home.png").write_bytes(adb("exec-out", "screencap", "-p"))
        tap(find_label(args.output, "Загрузки"))
        tap(find_label(args.output, TITLE))
        check_playback_state(args.output, "PLAYING", 3, require_progress=True)
        (args.output / "offline-playback.png").write_bytes(adb("exec-out", "screencap", "-p"))
        adb("shell", "cmd", "media_session", "dispatch", "pause")
        check_playback_state(args.output, "PAUSED", 2)
        adb("shell", "cmd", "media_session", "dispatch", "play")
        check_playback_state(args.output, "PLAYING", 3)
        print("Android clean install, launch, offline playback, pause and resume passed.")
    finally:
        (args.output / "android-runtime.log").write_bytes(adb("logcat", "-d", "-s", "AndroidRuntime"))


if __name__ == "__main__":
    main()
