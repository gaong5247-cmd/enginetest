from pathlib import Path
import re

manifest = Path("src-tauri/gen/android/app/src/main/AndroidManifest.xml")
if not manifest.exists():
    raise SystemExit(f"AndroidManifest.xml not found: {manifest}")

text = manifest.read_text(encoding="utf-8")
permission = '<uses-permission android:name="android.permission.INTERNET" />'
if permission not in text:
    match = re.search(r"<manifest\b[^>]*>", text)
    if not match:
        raise SystemExit("Invalid AndroidManifest.xml: <manifest> tag not found")
    pos = match.end()
    text = text[:pos] + "\n    " + permission + text[pos:]
    manifest.write_text(text, encoding="utf-8")
    print("Added INTERNET permission")
else:
    print("INTERNET permission already present")
