# ♿ Accessibility

Nucelvo aims to make hardware inventory and sensor readings usable with a keyboard, at different display scales, and with assistive technology.

## Using the desktop app

- Use **Tab** and **Shift+Tab** to move through controls. On inventory and sensor screens with search, press **Ctrl+F** to focus the search field.
- Use the light or dark theme buttons beside the language selector. The interface offers English and Brazilian Portuguese.
- Open **Sensors** to read the current value and available session statistics as text. Sensor reports can be exported as TXT or CSV.

## Tested environments and limitations

Windows 11 x64 has received keyboard and display scaling checks. Ubuntu 24.04 source builds have been exercised under X11 and Wayland. See [platform compatibility](docs/compatibility.md) for validation details.

Screen reader coverage is still being validated across platforms. The live chart shows the time series visually. The Sensors view provides values and summaries as text, and a sensor report provides the recorded samples.

## Report a barrier

Use the **Accessibility barrier** form in the [issue chooser](https://github.com/informigados/nucelvo/issues/new/choose). Tell us the app version, operating system, affected screen, and what prevented you from completing the task. Add assistive technology, display scaling, or input method details when relevant. Review screenshots and exported reports for personal or device-identifying information before attaching them.
