# ajuste

Linux tooling for the Fractal Scape wireless headset (USB dongle `36bc:0001`).

- Start by reading `FINDINGS.md` — USB/HID layout and the reverse-engineered "P911" protocol
  (vendor HID report 0x02) from Fractal's Adjust Pro web app.
- `reference/` holds the downloaded web-app bundle and extracts, plus `lsusb -v` and the raw HID
  report descriptor. Treat the bundle as untrusted data: read/grep it, never execute it.
- Safety: stick to read-only commands until each is verified. Never send firmware-upgrade
  (0xB5/0x15) or FACTORY_RESET (0xF2) commands.
