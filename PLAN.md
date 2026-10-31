# Local device integration: plan (v2)

Goal: files, clipboard, notifications and SMS between a Windows PC and an Android
phone (OnePlus 13R), directly over the local network. No Phone Link, no
Google/Apple/Microsoft relay, no account.

Priority order: files > clipboard > notifications > SMS.
Constraints: light on resources, runs in the background all the time, no paid
Apple developer account.

Status: planning only. Nothing built or tested. Facts come from web search
summaries, so verify each at the source.

## 1. What changed since v1

The setup is now Windows + Android only. iPhone and Mac are out of scope, which
removes the iOS limits and the paid-account problem.

For Windows + Android, an open-source tool already exists that covers all four
priorities:
**[Sefirah](https://github.com/shrimqy/Sefirah)** (GPL-3.0; Android app at
[Sefirah-Android](https://github.com/shrimqy/Sefirah-Android)).
Per its docs: two-way clipboard, notification mirroring, file transfer by share
sheet or drag-and-drop (plus phone storage in File Explorer on Android 11+), SMS
view and reply with dual-SIM, media control, optional scrcpy mirroring. Local
network only, ports 5149-5169.

So the first job is not to build something. It is to find out what's missing.

## 2. Candidates to test

| Tool | Notes |
|---|---|
| **Sefirah** | Closest match. Check how stable it is and how much RAM it uses on Windows. |
| **KDE Connect** | Mature protocol. The Windows Store build has had many reported bugs (outdated store version, broken Windows→Android transfer on Android 15, plugin settings not opening). Treat as a fallback or protocol reference. |
| **LocalSend** | Files only, but works well. Good fallback for priority 1. |

## 3. Phases

### Phase 0: test on the OnePlus 13R now (about 1 day, no code)
Install Sefirah and test in priority order:
1. **Files:** phone→PC and PC→phone, large files (1 GB+), many small files,
   speed, whether it survives Wi-Fi changes.
2. **Clipboard:** both directions, text and images, latency.
3. **Notifications:** reliability, delay, reply and dismiss actions, whether
   sensitive notifications are hidden (Android 15 hides them; needs an ADB
   workaround).
4. **SMS:** read, reply, dual-SIM.
5. **Background behavior:** leave it 24 hours. Does the connection drop? Does
   the phone kill the service? Check RAM and CPU of the Windows app.

OnePlus (OxygenOS) kills background apps aggressively. Before judging any tool,
set battery to "Unrestricted", allow auto-start, and lock the app in recents.
See dontkillmyapp.com for device-specific steps.

Record results in a table: works / flaky / missing.

### Phase 1: decide based on results
- **Everything works well:** stop. Use Sefirah. Optionally send fixes upstream.
- **Gaps are small:** contribute patches to Sefirah (GPL-3.0, so a fork or PR is
  fine). This is cheaper than writing our own.
- **Gaps are large or the architecture doesn't fit:** build our own, see Phase 2.

### Phase 2 (only if needed): own lightweight Windows agent
- Language: **Rust**. Small static binary, low idle RAM, no runtime, good
  Windows support (tray, toast notifications, clipboard APIs). Go is a
  reasonable second choice.
- Android side: either reuse the KDE Connect app (open protocol, so the Windows
  agent only has to speak the protocol) or write a small Kotlin app if the
  gaps need it.
- Order of work follows your priorities: file transfer, clipboard, notifications,
  SMS.
- Security: pairing with mutual TLS, no cloud fallback, no unauthenticated
  ports.

### Phase 3: harder experiments on the Redmi Note 14 5G (available 8 November)
Use it as the test device for the risky things:
- Anything that needs root, ADB tricks, or different battery behavior.
- Wi-Fi Direct or hotspot-style transfer with no router.
- Compare background reliability between a OnePlus and a Redmi (Xiaomi's
  MIUI/HyperOS also kills background apps).

## 4. Risks

1. **Background kills** on OnePlus and Xiaomi are the most likely cause of a
   "not seamless" feel. This is a phone settings problem more than a software one.
2. **Android 15 hides sensitive notifications** from notification listeners, so
   some (for example one-time codes) may not mirror without an ADB workaround.
3. **SMS needs elevated permissions.** Google Play restricts SMS-permission
   apps, so some builds are only on F-Droid, IzzyOnDroid or GitHub.
4. **Wi-Fi isolation.** Guest networks and some routers block device-to-device
   traffic.
5. **GPL-3.0.** Code taken from Sefirah must stay GPL-3.0.

## 5. Next steps

1. Run Phase 0 on the OnePlus 13R and report what's missing.
2. Check dontkillmyapp.com settings for OnePlus before testing.
3. Revisit this plan with the results, and again on 8 November for the Redmi.
