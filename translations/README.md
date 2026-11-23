# Nucelvo translations

English (US) is the default and fallback. Its source strings live in `@tr(...)` expressions in `crates/nucelvo-desktop/ui/main.slint`. The embedded Brazilian Portuguese catalog is `pt-BR/LC_MESSAGES/nucelvo.po`. The PO message context matches the Slint component that contains the expression.

Update the PO catalog when adding or changing a source string. Preserve placeholders, hardware names, and canonical units. Select each language in the app to check layout, accents, and number formatting. Run `python scripts/check-translations.py` to verify catalog coverage and placeholders. Run `cargo run --locked -- --smoke-test` to check language switching in the desktop window.

Inventory labels, reading states, source names, and human-readable report headings are defined in the collection, core, and report crates. Update both language variants when changing those strings. JSON and CSV keys stay stable when switching languages. The native save dialog uses the app language for its title and the operating system language for its built-in controls.
