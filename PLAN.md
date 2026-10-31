# Local device integration: plan

Goal: notifications, clipboard, file transfer and media/call control between
iPhone, Android, Mac and Windows, directly between devices. No Phone Link, no
Google/Apple/Microsoft relay, no account.

Status: planning only. Nothing is built, and nothing below was tested. Facts come
from web search summaries and one README, so verify each at the source before
committing to it.

## 1. Strategy

Don't rewrite what exists. Build one thin piece, a **desktop hub**, and reuse the
rest.

- Android side: stock **KDE Connect** (open protocol, mature, F-Droid).
- Desktop side: one small cross-platform daemon for Mac and Windows that speaks
  the KDE Connect protocol. KDE Connect's Mac and Windows clients are the weak
  link, so this is the main piece to write.
- iPhone side: iOS can't run a background service, so we use two channels:
  1. **BLE/ANCS** (no app on the phone) for notifications, calls and media.
  2. A **small native iOS app** for anything the user triggers: share sheet,
     clipboard push, file send and receive.
- Files: **LocalSend** works on all five platforms today. Use it as the baseline
  and integrate later only if worth it.

## 2. What iOS allows

These limits decide the plan.

| Capability | Possible? | How |
|---|---|---|
| iPhone notifications on desktop | Yes, read-only | ANCS over BLE. A Windows project does it already. |
| Reply to iPhone notifications | No | ANCS exposes only preset actions (answer/decline, etc.). |
| iPhone calls: see, answer, decline | Partly | ANCS action labels. Audio stays on the phone. |
| iPhone media control | Yes | AMS over BLE. |
| iPhone SMS read/send/reply | No | Notifications only. Bluetooth MAP is what Phone Link uses, and it's tied to the OS. |
| Clipboard iPhone → desktop | Only on a user action | Foreground app, share sheet, Shortcut, keyboard extension. No background read. |
| Clipboard desktop → iPhone | Yes, needs the app open | Push to the app, then set the clipboard. |
| Files both ways | Yes | App in the foreground. Wi-Fi Aware (iOS 26) may allow faster direct transfer. Unverified. |
| Remote lock, Find My, camera | No | |

Android↔desktop has none of these limits. KDE Connect already does notification
mirroring, SMS reply, clipboard, files and media.

## 3. Phases

### Phase 0: baseline, about 1 day, no code
Install KDE Connect on Android and desktop, plus LocalSend everywhere. Check what
already works. Try AirSync (MIT, Android↔Mac, same Wi-Fi) as a comparison for the
Mac side. Write down the actual gaps before building anything.

### Phase 1: desktop hub with the KDE Connect protocol (Android ↔ Mac/Windows)
- Implement the KDE Connect protocol in one codebase that runs on both desktops.
  That covers UDP/mDNS discovery, TLS with paired certificates, and the packet
  types.
- Packets to support first: `ping`, `notification`, `clipboard`, `share.request`.
- Language choice is open. Rust or Go give a single binary. Python is quickest
  to prototype.
- Stay compatible with the official Android app, so we don't write an Android app.
- Exit test: Android notifications appear on a Mac and a Windows PC, and the
  clipboard syncs both ways.

### Phase 2: iPhone notifications via BLE/ANCS (iPhone → desktop)
- Windows: start from [iphone-companion](https://github.com/312arjun/iphone-companion)
  (MIT, Python/bleak, ANCS + AMS + calls, no phone app). Fork or reuse its
  `ancs.py`/`ams.py`. Don't copy its UI.
- Mac: unproven. [ANCS-Mac](https://github.com/KhaosT/ANCS-Mac) is a 2014 demo
  and reportedly broken. **First task: a 1-2 day spike** to confirm whether
  CoreBluetooth on a current macOS can bond with an iPhone and subscribe to ANCS.
  If it can't, the fallback is an ESP32 relay or a Windows-only iPhone path.
- Feed ANCS events into the hub's notification pipeline, so iPhone and Android
  notifications look the same on the desktop.
- Known issues: a BLE bond only works with one client at a time (conflicts with
  Phone Link and other Bluetooth hosts), and iPhones rotate BLE addresses.

### Phase 3: small native iOS app (Swift)
- Needs an Apple developer account. A free account works for personal use but the
  signing expires every 7 days. A paid account ($99/yr) gives a year.
- Features: a share-sheet extension (send links, text and files to the desktop),
  a "send clipboard" action (App Intents / Shortcuts), a "receive clipboard and
  files" screen, and Bonjour discovery.
- Use the KDE Connect protocol if practical, so the hub has one protocol. The
  existing [kdeconnect-ios](https://github.com/KDE/kdeconnect-ios) is a starting
  point, but its status is unclear and it has reported bugs.
- Optional: a BLE background mode to wake the app when the desktop has something
  to send. Needs testing, since iOS limits background wakeups.

### Phase 4: polish
Pairing UX, a tray app, notification rules (quiet hours, app filters, one-time
code copy), per-device trust, and optional encrypted history.

## 4. Feature matrix after each phase

| | Android↔Mac | Android↔Win | iPhone→Mac | iPhone→Win |
|---|---|---|---|---|
| Notifications | Phase 1 | Phase 1 | Phase 2 (if spike works) | Phase 2 |
| Clipboard | Phase 1 | Phase 1 | Phase 3, manual | Phase 3, manual |
| Files | Phase 1 / LocalSend | Phase 1 / LocalSend | Phase 3 / LocalSend | Phase 3 / LocalSend |
| SMS reply | Phase 1 | Phase 1 | No | No |
| Media / calls | Phase 1 | Phase 1 | Phase 2 | Phase 2 |

Android↔Android and iPhone↔Android directly are out of scope for now.

## 5. Risks

1. **Mac ANCS may not work on current macOS.** This is the biggest unknown, so
   spike it first.
2. **iOS background limits** mean iPhone clipboard sync will always be manual or
   foreground only. Set that expectation up front.
3. **Single BLE bond.** The iPhone's notification bond is exclusive.
4. **Maintenance of the KDE Connect iOS app.** The project has had crashes and
   one-directional transfer bugs. May need our own app.
5. **Apple signing** costs money or time (see Phase 3).
6. **Security.** Pairing must use mutual TLS or equivalent, with no cloud fallback.
   Review before any real use.

## 6. Decisions needed

1. Which phones and computers do you actually use? That decides whether to start
   with Windows or Mac.
2. Which features matter most: notifications, clipboard, files, or SMS?
3. Language preference for the hub (Rust, Go, Python, other).
4. Are you willing to pay for an Apple developer account?
5. Should this be open source from the start?

## 7. Suggested first steps

1. Phase 0 install and test (no code).
2. The Mac ANCS spike (decides the iPhone→Mac path).
3. Phase 1 prototype: `ping` and `notification` packets in the desktop hub.
