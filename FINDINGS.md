# Fractal Scape headset dongle — protocol findings

Gathered 2026-11-08. Goal: control/read the Fractal Scape wireless headset from Linux
(battery, mute, EQ, sidetone, ANC…) without Fractal's web app.

Nothing below has been sent to the device yet — the protocol was derived by reading
Fractal's web configurator source. Verify each command before relying on it.

## 1. The USB device

- **Fractal Scape Dongle**, USB `36bc:0001` (Fractal VID 0x36BC = 14012), bcdDevice 3.21, USB 2.0 full speed
- On this machine: sysfs `1-1.4.4.1` (behind the Thunderbolt dock hub) — path may change
- Serial: `00000000911AD5BB1119`
- Raw dumps: `reference/lsusb-v.txt`, `reference/dongle_report_descriptor.hex`

### Interfaces
| If | Class | Driver | What |
|---|---|---|---|
| 0 | Audio control (UAC1) | snd-usb-audio | "Headset" terminals, volume/mute feature units |
| 1 | Audio streaming | snd-usb-audio | Playback: stereo, S16_LE or S24_3LE, 48/96 kHz |
| 2 | Audio streaming | snd-usb-audio | Capture: mono S16_LE, 16/48 kHz |
| 3 | HID | usbhid | `/dev/hidraw6` (number may change), input devices |

ALSA card id: `Dongle` (was card 2).

### HID report descriptor (decoded by hand)
| Report ID | Usage page | Contents |
|---|---|---|
| `0x0C` | Consumer | Input bits: Vol+, Vol−, Mute, Play/Pause, Next, Prev |
| `0x05` | Telephony (Headset) | Inputs: Hook switch, Phone mute, Redial, Flash, Line busy tone, Line, Speed dial, 1 programmable button. Outputs (LED page): Off-hook, Mute, Ring, Hold, Microphone, On-line |
| `0x06` / `0x07` | Vendor `0xFF13` | 61-byte output / 61-byte input — **not used by Fractal's app** (likely BT chip's own channel) |
| `0x02` | Vendor `0xFF00` | 63-byte output / 63-byte input — **Fractal's "P911" control protocol** |

`/dev/hidraw*` is root-only by default → need a udev rule, e.g.
`SUBSYSTEM=="hidraw", ATTRS{idVendor}=="36bc", ATTRS{idProduct}=="0001", MODE="0660", TAG+="uaccess"`

## 2. Fractal Adjust Pro web app

- https://adjust.fractal-design.com/ — React + MUI SPA using **WebHID**
- Supports: Scape headset, Scape BT dongle, Adjust Pro hub, Radius pump controller
- Bundle saved: `reference/main.e2372c083cc6b94f.js` (6.3 MB, minified but class/constant names intact)
- Extracts: `reference/headset.js` (class `Headset`; tail spills into hub class),
  `reference/dongle.js` (class `BTDongle`), `reference/base.js` (class `P911Base`)
- Useful anchors to grep in the bundle: `class P911{}`, `class VendorIDs`, `class ProductIDs`,
  `getUpdatedDeviceState`, `sideToneControl`, `SetEqSettings`, `detectDeviceType`

### IDs from the app
| | VID | PID |
|---|---|---|
| Headset (direct USB) | 0x36BC | 0x2001 (8193) |
| Dongle | 0x36BC | 0x0001 |
| Hub | 0x36BC | 0x1002 (also dev 0x1001) |
| Radius | 0x36BC | 0x1003 |
| Headset dev | 0x0E8D (MediaTek) | 0x080B |
| Dongle dev | 0x0E8D (MediaTek) | 0x080A |

WebHID filter: `usagePage 0xFF00, usage 0x01`.

## 3. P911 protocol (report ID 0x02)

**Request:** `sendReport(0x02, [deviceType, cmd, args…])` (pad to 63 bytes on hidraw: `02 <type> <cmd> …`)
**Response:** input report 0x02, data = `[deviceType, cmd, status(0=OK), payload…]`
(GET_EQ_SETTINGS skips the status check). One transaction at a time; app timeout default, 2 s for some.

Commands the app expects a response for: all FACTORY_* except SET_WDL_SWITCH, all EQ cmds listed,
FW request/send/commit (not reboot), all PWM, most EFFECT cmds. STREAMING (0xA5) has no response.

**The dongle forwards headset commands** over Bluetooth: the app creates a `Headset` delegate on the
dongle's HID device and uses headset device-type bytes (0xF1, 0xA7…). Dongle-only commands use 0x11.

### Device types
| Const | Value |
|---|---|
| DEVICE_TYPE_FACTORY_DONGLE | 0x11 |
| DEVICE_TYPE_FACTORY_HEADSET | 0xF1 |
| DEVICE_TYPE_EQ | 0xA7 |
| DEVICE_TYPE_EFFECT (lighting) | 0xA4 |
| DEVICE_TYPE_STREAMING | 0xA5 |
| DEVICE_TYPE_PWM (hub fans) | 0xD1 |
| DEVICE_TYPE_FW_UPGRADE_HEADSET | 0xB5 |
| DEVICE_TYPE_FW_UPGRADE_DONGLE | 0x15 |

### Factory/info commands (with 0x11 dongle or 0xF1 headset)
| Cmd | Value | Notes |
|---|---|---|
| GET_FW_VERSION | 0x01 | 3 bytes |
| GET_SERIAL_NUMBER_MSB / MLB | 0x02 / 0x03 | 17 bytes each; revision at serial offset 10 (2 chars) |
| GET_EDITION_ID | 0x04 | 1 byte |
| GET_BT_ADDR | 0x05 | 6 bytes (headset) |
| GET_WDL_SWITCH / SET_WDL_SWITCH | 0x14 / 0x15 | Windows Dynamic Lighting opt-in |
| IS_MEMORY_OK | 0x1C | |
| GET_DEVICE_STATE | 0x21 | see below |
| SIDE_TONE_CONTROL | 0x34 | `F1 34 <state> <vol>` → resp[3]=state, resp[4]=vol |
| ENC_CONTROL (ANC/ENC) | 0x36 | `F1 36 <1|0>` |
| FACTORY_RESET | 0xF2 | ⚠ avoid |
| INITIALIZE / SHUTDOWN | 0xF0 / 0xF1 | |

**Dongle state** `11 21` → resp[3] == 1 ⇒ headset connected.

**Headset state** `F1 21` → response bytes:
| Byte | Field |
|---|---|
| 3 | boom mic attached (0 = yes) |
| 4 | muted (0 = yes) |
| 5 | selected EQ slot |
| 6 | selected lighting slot |
| 7 (8) | chat volume (app's high-byte read uses `>>8` → always 0; bug) |
| 9 (10) | game volume (same bug) |
| 11 | volume state |
| 12 | BT mode |
| 13 | hall sensor |
| **14** | **battery %** |
| 15 | sidetone on (1) |
| 16 (17) | sidetone volume |
| 18 | BT connection state |
| 19 | ENC on |
| 20 | power state |
| 33 | dynamic lighting off (0 = on, if WDL opted in) |
| 40 | WDL opt-in |

### EQ (0xA7)
| Cmd | Value | Format |
|---|---|---|
| SET_EQ_SETTINGS | 0x01 | `A7 01 (profileID<<4 | sampleRate&0xF)` then per point: `[enabled, filterType, freqLo, freqHi, gain(4B), Q(4B)]` (w2 = probably float32 LE); unused points `[0, Peak, 0x20,0x00, 0.0, 0.5]` |
| SET_EQ_COEF | 0x02 | `A7 02 profileID idx 0 …coefs` |
| COMMIT_EQ | 0x03 | `A7 03 profileID <t> … len presetIdString` |
| GET_EQ_SETTINGS | 0x04 | `A7 04 slot 0` (0 = EQ data, 1 = metadata) |
| GET_EQ_COEF | 0x05 | |
| SWITCH_EQ_MODE | 0x06 | |
| SELECT_EQ_MODE | 0x07 | `A7 07 slot` (3 slots, 1-based) |
| SET/COMMIT/GET_MIC_EQ | 0x11 / 0x12 / 0x13 | commit arg 1 = save, 2 = erase; default mic = `CupMicWB` |

Unclear: `L.w2` serializer (assume float32 LE), filter-type enum `o.Fe`, mic enum `o.hl`, sidetone enum `o.r5`.

### Firmware (0xB5 headset / 0x15 dongle) — ⚠ do not touch casually
REQUEST 0x01, SEND_DATA 0x02, COMMIT 0x03, REBOOT 0x04. Firmware list fetched as JSON
(`firmware_list[]` with vid/pid/url/revision).

## 4. Next steps (proposed)
1. udev rule for hidraw access
2. Python read-only tool: open hidraw by VID/PID + interface 3 / usage page 0xFF00, send `02 11 21`,
   then `02 F1 21`, print battery/mute/mic/EQ slot/sidetone/ANC
3. Then add safe setters: sidetone, ENC, EQ slot
4. Optional: watch unsolicited input reports; probe the unused 0xFF13 channel

## 5. Additional details (bundle research, 2026-11-08), now implemented in `crates/scape-proto`

- **Transport:** the app takes the *first* input report as the reply, with no filtering.
  We instead match on `[type, cmd]` and skip other reports.
  - The status check is `data[2]==0`; GET_EQ_SETTINGS is exempt.
  - The timeout is 2 s.
  - SELECT_EQ_MODE (0x07) and SWITCH_EQ_MODE (0x06) get **no reply**.
- **Polling:** every 1500 ms, `11 21` then (if the headset is connected) `F1 21`.
  Unsolicited reports are not handled.
- **`w2`** is float32 LE.
- **SET_EQ_SETTINGS** is always 63 bytes: `A7 01 (id<<4|7)` + 5 × `[en, type, freqLE16, gain f32, Q f32]`.
  - Unused points are `[0, 0, 0x20,0x00, 0.0, 0.5]`.
  - A flat profile is a single enabled point at 22000 Hz (`F0 55`).
- **GET_EQ_SETTINGS:** `A7 04 slot 0|1`.
  - The reply mirrors the SET layout. Points with `en=0` or `freq>20000` are skipped.
  - The metadata reply (arg 1) is byte 3 = length, then the preset-UID string.
  - If slot 2 or 3 reads `data[3]==0`, the app reads slot 4 or 5 (factory copies).
- **SET_EQ_COEF:** `A7 02 id rate idx b0 b1 b2 a1 a2`, all f32 LE, normalised so a0 = 1.
  - Sent per point, then per rate (1 = 44k, 2 = 48k, 4 = 96k).
  - Coefficients are RBJ cookbook, computed client-side at **44000**/48000/96000 Hz.
  - Verified against the coefficients the bundle ships for its presets
    (`tests/preset_coefs.rs`).
  - An empty profile sends fixed pass-through coefficients at idx 0.
- **COMMIT_EQ:** `A7 03 id mode 00000000 len uid`. Mode: 1 = live (not persisted), 2 = save,
  3 = erase. The app sends commits in order settings → coefficients → commit, and runs a
  live update 50 ms after each editor change.
- **Filter types:** Peak 0, LowShelf 1, HighShelf 2, LowPass 3, HighPass 4, Notch 5, BandPass 6.
  The editor offers peak and the two shelves, with gain ±10 dB, 20–20000 Hz and Q 0.5–10.
- **Presets:**
  - Balance (flat), uid `7d1401c3-fcbc-6c29-a2t2-eba4ae4d6bd8`
  - Clarity: LS 265 Hz / −2.3 dB / Q 0.5, PK 380 / +2.2 / 1.45, HS 1470 / +1.2 / 1.133
  - Depth: LS 120 / +2.5 / 0.975, PK 260 / −2 / 1.75
- **Mic EQ:** a dB magnitude table, not coefficients.
  - The PEQ is evaluated at 96 kHz at `31.25*(k+1)` Hz for k = 0..255, with Q ≥ 0.5.
    Values are clamped to −20..18 dB.
  - It is sent as 32 chunks: `A7 11 mic chunk 8×f32`.
  - Commit is `A7 12 01 mic`; erase is `A7 12 02 01`; get is `A7 13 mic chunk`.
  - The app always uses mic type 1 (CupMicWB); SWB types use 48 chunks.
- **Sidetone:** `F1 34 op amount`, with op 0 = disable, 1 = enable, 2 = vol up, 3 = vol down.
  - The reply is `F1 34 00 state vol`. The raw volume is 0 or 55–100 in steps of 5, and
    UI% = (raw−50)×2.
  - Amounts of 51 or more are split into 50 + the remainder.
  - On mount the app sends `F1 34 01 01`. It never sends disable.
- **ENC:** `F1 36 0|1`, status-only reply. Neither command has a getter; use the state bytes
  15/16 (sidetone) and 19 (ENC).
- **Not in the app:** setting volume, mute or power (these are read-only state).
- **Lighting (0xA4/0xA5):** client-rendered frames are uploaded per slot. Slot 0 means off.
  This is not implemented here.

## 6. Verified on hardware (2026-11-08)

Dongle fw 3.33.0, headset fw 3.48.0.

| Command | Result |
|---|---|
| `11 21` / `F1 21` | ✅ Works. Payload offsets match §3 (battery = byte 14). |
| State byte 3 | ✅ **1 = boom attached**. Adjust Pro's naming suggested the opposite. Not yet re-checked with the boom unplugged. |
| State byte 4 | ✅ **1 = muted**. Raising the boom sets it along with hall byte 13 = 1. Muting another way (the user muted by hand) sets byte 4 only, with hall = 0. |
| `11/F1 01..05` info | ✅ The serial is NUL-terminated ASCII from byte 3, 20 characters. The dongle returns it for both halves; the headset's second half is empty. |
| `A7 04` GET_EQ | ✅ Byte 2 is `id<<4\|7` for data. For metadata (arg 1), byte 2 is the slot number. User preset IDs can be 21-character nanoids rather than UUIDs. Slot 2 matches our Clarity encoding byte for byte. |
| `F1 36` ENC | ✅ Both directions work; status-only reply; byte 19 follows. |
| `A7 07` SELECT_EQ | ✅ No reply; byte 5 follows. |
| `F1 34` sidetone | ✅ VolUp and VolDown work; the reply `F1 34 00 state vol` carries the new raw value. **Disable (op 0) works** (byte 15 → 0). Enable with amount 1 does *not* change the volume. |

| `A7 01/02/03` EQ write, live (mode 1) | ✅ Audible right away, and the stored slot data is unchanged. The COMMIT reply carries extra bytes after the status (`03 00 00 00 01 00 17 00 02 00 90`; meaning unknown). |
| EQ write, save (mode 2) | ✅ Reads back exactly, preset UID included. Slot 3 was restored to its original bytes and UID afterwards. |
| `A7 11` + `A7 12 01 01` mic EQ | ✅ 32 chunks plus the commit. The `A7 13` reply is `A7 13 00 <chunk> 8×f32`, and the stored floats are bit-identical to our table. |
| `A7 12 02 01` mic erase | ✅ The table reads back as all zeros (the original state on this headset). |

Tests used slot 3. Slot 1 holds the user's own custom EQ (`Sv7n2oVHjUk6av7JenMs0`) and was never written.

### Physical controls observed in `F1 21` (2026-11-08)

| Action | Bytes that change |
|---|---|
| Flip boom up (mute) | b4 → 1, b13 (hall) → 1, b38 → 1 |
| Flip boom down | b4, b13 and b38 → 0 |
| Mute button press | b4 → 1, b38 → 1; hall unchanged. Toggles. |
| Mute button **hold** | b19 (ENC) toggles. Mute is unchanged. |
| Unplug boom | b3 → 0; auto-mutes (b4 and b38 → 1) |
| Unmute with boom unplugged | b4 and b38 → 0; the headset switches to its built-in omnidirectional mic |
| Plug boom back in | b3 → 1; no auto-mute |
| Put in charging dock | The headset leaves the radio link; the dongle's `11 21` byte 3 → 0. No state is readable while docked. |
| `A4 04 0\|1` lights off/on | ✅ Off goes dark, and on brings back the same stored effect. If the stored effect is "RGB Off", "on" stays dark (b6 still reads 1). |
| Lighting button | b6 toggles 1 ↔ 0 (slot 0 = lights off, matching Adjust Pro's `turnLight` = select slot 0/1) |
| EQ button | b5 cycles 1 → 2 → 3 → 1; **b25 mirrors b5** (so b25 is not part of the voltage field) |
| Switch to Bluetooth mode | Same as docking: the headset leaves the dongle (`11 21` byte 3 → 0). It reconnects ~14 s after switching back. So b12 "BT mode" can't be observed through the dongle. |
| Crown (click, double/triple click, rotate) | Nothing in `F1 21`. Sent as consumer-page report 0x0C and handled by the OS. Our transport skips these reports while waiting for replies, and polls were unaffected. |
| Power button (off) | **b30 → 1** about 1 s before the headset leaves the link (a shutdown-pending flag), then it drops like the dock case. |
| Take out of dock | Reconnects in ~20 s. The state is identical to before docking, with no "was charging" flag. |

- b4 and b38 both read as "muted", whatever the cause. b13 is the raw boom position.
- **b26–27, little-endian 16-bit**, is most likely the battery voltage in mV (e.g. `e0 0f` = 4064 at 91%,
  `ec 0f` = 4076 at 92%). It drifts by a few mV between polls.
- b7 = 0x99 and b8 = 0xff (Adjust Pro's "chat volume") stayed constant all session, but **reset to 00 00 after a
  BT-mode round trip**. They don't follow crown volume changes. Meaning unknown.
