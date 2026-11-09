# Ajuste

Native Linux control for the **Fractal Scape** wireless headset, replacing Fractal's
Chrome-only (WebHID) Adjust Pro web app.

| Crate | What |
|---|---|
| `crates/scape-proto` | Pure encoder/decoder for the P911 HID protocol (no I/O, unit-tested) |
| `crates/scape-hid` | hidraw transport + polling worker thread |
| `crates/ajustectl` | CLI |
| `crates/ajuste` | gpui-kit GUI with tray icon and low-battery notifications |

Protocol notes: [`FINDINGS.md`](FINDINGS.md).

## Install

**Arch Linux** (AUR):

```sh
yay -S ajuste-git        # or: paru -S ajuste-git
```

**Other distributions** (from source; needs Rust/cargo):

```sh
make
sudo make install        # PREFIX=/usr/local by default; the udev rule goes to /etc/udev/rules.d
```

Either way, replug the dongle once so the udev rule takes effect. It gives the logged-in user
access to the headset; no root is needed afterwards.

**Start in the tray at login:**

```sh
cp /usr/share/ajuste/ajuste-autostart.desktop ~/.config/autostart/   # /usr/local/share/… for source installs
```

The tray icon needs a StatusNotifierItem host. KDE has one built in; on GNOME install the
"AppIndicator and KStatusNotifierItem Support" extension.

## CLI

```sh
ajustectl status [--json]          # battery, mic, EQ slot, sidetone, ENC
ajustectl info                     # firmware / serial / BT address
ajustectl watch
ajustectl enc on|off
ajustectl lights on|off             # LEDs off/on (keeps the stored effect)
ajustectl lights-show                # decode the stored effect (e.g. "Sunset")
ajustectl lights-presets             # 17 built-in themes (still, shift, two-colour fade)
ajustectl lights-set "Northern lights" --brightness 60
ajustectl sidetone on --level 40   # 10 % steps
ajustectl eq presets
ajustectl eq show [1-3]
ajustectl eq select 2
ajustectl eq apply 1 --preset clarity            # live preview, not persisted
ajustectl eq apply 1 --file my.toml --save       # persist to slot
```

Add `--hex` to any command to see the raw reports, and `--dry-run` to the EQ writers
to print the reports without sending anything. EQ profile files:

```toml
[[points]]
filter = "lowshelf"   # peak | lowshelf | highshelf | lowpass | highpass | notch | bandpass
freq = 120
gain = 2.5
q = 0.975
```

## GUI

`ajuste [--hidden] [--fake-battery PCT]`. Closing the window keeps the tray icon; quit
from the tray menu. Settings and saved presets: `~/.config/ajuste/config.toml`
(`low_battery`, `critical_battery`, `start_hidden`, `presets`).

## License

MIT OR Apache-2.0, at your option.

## Safety

`scape-proto` cannot encode firmware-upgrade (0xB5/0x15), FACTORY_RESET (0xF2),
INITIALIZE/SHUTDOWN, lighting streaming, boot/timestamp effects, or uploads to any slot other than 1.
A unit test enforces this.

## Hardware status

Tested on a real Fractal Scape (dongle firmware 3.33.0, headset firmware 3.48.0), both through the 2.4 GHz dongle and over USB-C.
Every feature Ajuste exposes has been sent to the device and its result confirmed, by reading the
state back and by ear or eye:

| Feature | Status |
|---|---|
| Status: battery, mic boom, mute, EQ slot, sidetone, ENC, lights | ✅ |
| Firmware and serial numbers | ✅ |
| ENC on/off, sidetone on/off and level, EQ slot select | ✅ |
| Headset EQ: read, live preview, save | ✅ Tested on a spare slot |
| Lighting: on/off, read the stored effect, upload 17 presets | ✅ Byte-identical to Adjust Pro's uploads |
| Physical buttons, dock, Bluetooth mode, power-off | ✅ Reflected in state; see FINDINGS §6 |
| Mic EQ | ❌ Stored by the headset but no audible effect; removed |
| Wired USB-C mode (`36bc:2001`) | ✅ Reads, settings and lighting upload; see FINDINGS §9 |

[`FINDINGS.md`](FINDINGS.md) has the protocol and test details (§6–9).
