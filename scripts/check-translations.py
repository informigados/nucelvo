"""Check the embedded Brazilian Portuguese catalog against the desktop UI."""

import ast
import re
from pathlib import Path
from string import Formatter

ROOT = Path(__file__).resolve().parents[1]
STRING = r'"(?:[^"\\]|\\.)*"'


def po_value(block, key):
    match = re.search(rf"^{key} ({STRING})((?:\n{STRING})*)", block, re.MULTILINE)
    if not match:
        raise ValueError(f"Missing {key} in catalog entry")
    return ast.literal_eval(match[1]) + "".join(
        ast.literal_eval(line) for line in match[2].splitlines() if line
    )


def placeholders(text):
    return sorted(key for _, key, _, _ in Formatter().parse(text) if key is not None)


def main():
    ui = ROOT / "crates/nucelvo-desktop/ui/main.slint"
    catalog = ROOT / "translations/pt-BR/LC_MESSAGES/nucelvo.po"
    messages = {
        ast.literal_eval(message)
        for message in re.findall(rf"@tr\(\s*({STRING})", ui.read_text(encoding="utf-8-sig"))
    }
    translations = {}
    for block in re.split(r"\n\s*\n", catalog.read_text(encoding="utf-8")):
        if not block.strip():
            continue
        message = po_value(block, "msgid")
        if not message:
            continue
        translated = po_value(block, "msgstr")
        if po_value(block, "msgctxt") != "MainWindow" or message in translations:
            raise ValueError(f"Unexpected context or duplicate entry for {message!r}")
        if not translated.strip() or placeholders(message) != placeholders(translated):
            raise ValueError(f"Missing translation or mismatched placeholders for {message!r}")
        if (
            "\ufffd" in translated
            or re.search(r"\w\?\w", translated)
            or re.search(r"^#,.*\bfuzzy\b", block, re.MULTILINE)
        ):
            raise ValueError(f"Unreviewed or invalid translation for {message!r}")
        translations[message] = translated

    missing = messages - translations.keys()
    outdated = translations.keys() - messages
    if missing or outdated:
        raise ValueError(f"Missing messages: {sorted(missing)}. Outdated messages: {sorted(outdated)}.")
    print(f"Verified {len(messages)} Brazilian Portuguese UI messages and their placeholders.")


if __name__ == "__main__":
    main()
