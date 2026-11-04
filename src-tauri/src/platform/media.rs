//! Controls whatever is playing media on this computer (spec §9.4 `music.control` and
//! `music.now_playing`). No login and no network: it talks to the operating system.
//!
//! Windows: the system media session (what the volume flyout and the lock screen use), so
//! pause, play, next and previous reach Spotify, a browser tab or any player that
//! registers, and "now playing" comes from the same session. Volume and mute press the
//! system volume keys.
//!
//! macOS: the Spotify and Music apps through fixed AppleScripts run by `/usr/bin/osascript`
//! (macOS has no public "current media session"; browsers and other players are not
//! reached). The first use makes macOS ask the person to allow JARVIS to control the app.
//! Volume and mute use the system volume setting and need no permission.
//!
//! Other systems (Linux: MPRIS later) return `Unsupported`, so nothing pretends to work.

/// What can be asked of the media session. Fixed list: no text from anyone reaches the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    /// The system mute key toggles: pressing it again unmutes.
    MuteToggle,
}

/// The track the system says is current. The strings come from the playing app, so they
/// are untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaError {
    /// This operating system has no adapter yet (only built on the systems without one).
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    Unsupported,
    /// No app has a media session open.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    NothingPlaying,
    /// The app has a session but did not accept the command (only Windows reports this).
    #[cfg_attr(not(windows), allow(dead_code))]
    Refused,
    /// macOS has not let JARVIS control that app (Automation permission).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NotAllowed,
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    Failed,
}

/// Blocking: the tool calls it on a blocking thread.
pub fn send(cmd: Command) -> Result<(), MediaError> {
    imp::send(cmd)
}

/// Starts one Spotify track in the Spotify desktop app on this computer, without the
/// internet account route. `uri` must be a plain `spotify:track:` address; anything else is
/// refused before a script is run. macOS only; other systems say `Unsupported`. Never starts
/// Spotify itself: if it is not running the answer is `NothingPlaying`. Blocking.
pub fn play_spotify_uri(uri: &str) -> Result<(), MediaError> {
    imp::play_spotify_uri(uri)
}

/// Starts the Spotify desktop app on this computer so it can show up as a device to play on.
/// Windows only (macOS never starts Spotify, by design; other systems say `Unsupported`). It
/// refuses, without opening any dialog, when Spotify is not installed. The only thing ever passed
/// to the system is the fixed address `spotify:`. Blocking.
pub fn start_spotify_app() -> Result<(), MediaError> {
    spotify_start::start()
}

/// `Ok(None)` means no app has a media session. Blocking.
pub fn now_playing() -> Result<Option<NowPlaying>, MediaError> {
    imp::now_playing()
}

#[cfg(windows)]
mod spotify_start {
    use super::MediaError;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Registry::{
        HKEY_CLASSES_ROOT, KEY_READ, RegCloseKey, RegOpenKeyExW,
    };

    /// True if a program has registered the `spotify:` address scheme (the desktop app does).
    /// Without this check Windows would ask "how do you want to open this?" when it is missing.
    fn registered() -> bool {
        let name: Vec<u16> = "spotify\0".encode_utf16().collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call, `key` is a
        // valid out-pointer, and the key is closed straight away when it was opened.
        unsafe {
            let rc = RegOpenKeyExW(HKEY_CLASSES_ROOT, name.as_ptr(), 0, KEY_READ, &mut key);
            if rc == 0 {
                RegCloseKey(key);
                true
            } else {
                false
            }
        }
    }

    /// `rundll32.exe` by its full path under the Windows folder, so a program of that name in the
    /// current folder or on the path is never run instead.
    fn rundll32() -> std::path::PathBuf {
        match std::env::var_os("SystemRoot") {
            Some(root) => std::path::Path::new(&root)
                .join("System32")
                .join("rundll32.exe"),
            None => "rundll32.exe".into(),
        }
    }

    pub fn start() -> Result<(), MediaError> {
        if !registered() {
            return Err(MediaError::Unsupported);
        }
        let status = Command::new(rundll32())
            .arg("url.dll,FileProtocolHandler")
            .arg("spotify:")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            _ => Err(MediaError::Failed),
        }
    }
}

#[cfg(not(windows))]
mod spotify_start {
    use super::MediaError;

    pub fn start() -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
    }
}

#[cfg(windows)]
mod imp {
    use super::{Command, MediaError, NowPlaying};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_VOLUME_DOWN,
        VK_VOLUME_MUTE, VK_VOLUME_UP,
    };

    /// One volume command moves the volume by this many key presses (each is about 2%).
    const VOLUME_PRESSES: usize = 5;

    /// The media session API needs a COM apartment on the calling thread, and a pooled
    /// thread may not have one, so the work runs on a short-lived thread of its own.
    fn in_apartment<T: Send + 'static>(
        f: impl FnOnce() -> Result<T, MediaError> + Send + 'static,
    ) -> Result<T, MediaError> {
        std::thread::Builder::new()
            .name("media-control".into())
            .spawn(move || {
                // SAFETY: balanced by CoUninitialize below whenever it succeeded.
                let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let out = f();
                if init.is_ok() {
                    unsafe { CoUninitialize() };
                }
                out
            })
            .map_err(|_| MediaError::Failed)?
            .join()
            .map_err(|_| MediaError::Failed)?
    }

    fn current_session() -> Result<Session, MediaError> {
        let manager = Manager::RequestAsync()
            .and_then(|op| op.join())
            .map_err(|_| MediaError::Failed)?;
        // No session at all comes back as an error from the system.
        manager
            .GetCurrentSession()
            .map_err(|_| MediaError::NothingPlaying)
    }

    fn accepted(r: windows::core::Result<bool>) -> Result<(), MediaError> {
        match r {
            Ok(true) => Ok(()),
            Ok(false) => Err(MediaError::Refused),
            Err(_) => Err(MediaError::Failed),
        }
    }

    fn press(vk: u16, times: usize) -> Result<(), MediaError> {
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut inputs = Vec::with_capacity(times * 2);
        for _ in 0..times {
            inputs.push(key(0));
            inputs.push(key(KEYEVENTF_KEYUP));
        }
        // SAFETY: `inputs` is a valid array of INPUT structures and the size is that of one.
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err(MediaError::Failed)
        }
    }

    pub fn send(cmd: Command) -> Result<(), MediaError> {
        match cmd {
            Command::VolumeUp => press(VK_VOLUME_UP, VOLUME_PRESSES),
            Command::VolumeDown => press(VK_VOLUME_DOWN, VOLUME_PRESSES),
            Command::MuteToggle => press(VK_VOLUME_MUTE, 1),
            Command::Play | Command::Pause | Command::Next | Command::Previous => {
                in_apartment(move || {
                    let s = current_session()?;
                    accepted(
                        match cmd {
                            Command::Play => s.TryPlayAsync(),
                            Command::Pause => s.TryPauseAsync(),
                            Command::Next => s.TrySkipNextAsync(),
                            _ => s.TrySkipPreviousAsync(),
                        }
                        .and_then(|op| op.join()),
                    )
                })
            }
        }
    }

    pub fn play_spotify_uri(_: &str) -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
    }

    pub fn now_playing() -> Result<Option<NowPlaying>, MediaError> {
        in_apartment(|| {
            let s = match current_session() {
                Ok(s) => s,
                Err(MediaError::NothingPlaying) => return Ok(None),
                Err(e) => return Err(e),
            };
            let playing = s
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus())
                .map(|st| st.0 == Status::Playing.0)
                .unwrap_or(false);
            let props = s
                .TryGetMediaPropertiesAsync()
                .and_then(|op| op.join())
                .map_err(|_| MediaError::Failed)?;
            let text = |h: windows::core::Result<windows::core::HSTRING>| {
                h.map(|h| h.to_string_lossy()).unwrap_or_default()
            };
            let (title, artist) = (text(props.Title()), text(props.Artist()));
            if title.is_empty() && artist.is_empty() {
                return Ok(None);
            }
            Ok(Some(NowPlaying {
                title,
                artist,
                playing,
            }))
        })
    }
}

/// The logic of the macOS adapter, kept free of anything that runs a process so it is tested
/// on every system. Every script is a compile-time constant or built only from the constants
/// below: no text from the model, a song title or the user ever becomes part of a script.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod applescript {
    use super::{Command, MediaError, NowPlaying};

    /// The two players macOS exposes to scripting. Their names are fixed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Player {
        Spotify,
        Music,
    }

    impl Player {
        /// Both players, for the tests that must cover each of them.
        #[cfg(test)]
        pub const ALL: [Player; 2] = [Player::Spotify, Player::Music];

        /// The application name inside a script.
        pub fn app(self) -> &'static str {
            match self {
                Self::Spotify => "Spotify",
                Self::Music => "Music",
            }
        }

        /// The process name `pgrep -x` looks for. Checking by process means a player that is
        /// not running is never launched, and an app that is not installed is never named in
        /// a script (which could make macOS ask "Where is it?").
        pub fn process(self) -> &'static str {
            self.app()
        }
    }

    /// What a running player is doing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum State {
        Stopped,
        Paused,
        Playing,
    }

    /// The separator between fields of one answer. A control character, so a title or an
    /// artist cannot contain it (it is also dropped from the text before display).
    pub const SEP: char = '\u{1f}';

    pub fn state_script(p: Player) -> String {
        format!(
            "tell application \"{}\" to return player state as string",
            p.app()
        )
    }

    pub fn now_playing_script(p: Player) -> String {
        format!(
            "tell application \"{}\"\n\
             set t to name of current track\n\
             set a to artist of current track\n\
             return t & (ASCII character 31) & a & (ASCII character 31) & (player state as string)\n\
             end tell",
            p.app()
        )
    }

    /// Play, pause, next, previous: aimed at one player. `None` for the volume commands,
    /// which belong to the system and not to a player.
    pub fn transport_script(p: Player, cmd: Command) -> Option<String> {
        let verb = match cmd {
            Command::Play => "play",
            Command::Pause => "pause",
            Command::Next => "next track",
            Command::Previous => "previous track",
            Command::VolumeUp | Command::VolumeDown | Command::MuteToggle => return None,
        };
        Some(format!("tell application \"{}\" to {verb}", p.app()))
    }

    /// Volume moves by this much (out of 100) per command, about what five key presses do.
    pub const VOLUME_STEP: u8 = 10;

    /// The system volume and mute. `None` for the transport commands.
    pub fn system_script(cmd: Command) -> Option<String> {
        match cmd {
            Command::VolumeUp => Some(volume_script(VOLUME_STEP as i16)),
            Command::VolumeDown => Some(volume_script(-(VOLUME_STEP as i16))),
            Command::MuteToggle => {
                Some("set volume output muted (not (output muted of (get volume settings)))".into())
            }
            _ => None,
        }
    }

    fn volume_script(delta: i16) -> String {
        // Clamped to 0..100 inside the script: an out-of-range value is an error otherwise.
        format!(
            "set v to (output volume of (get volume settings)) + ({delta})\n\
             if v > 100 then set v to 100\n\
             if v < 0 then set v to 0\n\
             set volume output volume v"
        )
    }

    /// A Spotify track address: `spotify:track:` and exactly 22 letters or digits. Nothing
    /// else can be passed to the play-a-track script.
    pub fn valid_track_uri(uri: &str) -> bool {
        uri.strip_prefix("spotify:track:")
            .is_some_and(|id| id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
    }

    /// Plays one track in Spotify. The address is **an argument** of the script (`argv`),
    /// never part of its text, so it cannot change what the script does.
    pub const PLAY_TRACK_SCRIPT: &str = "on run argv\n\
         tell application \"Spotify\" to play track (item 1 of argv)\n\
         end run";

    /// What `player state as string` says.
    pub fn parse_state(out: &str) -> State {
        match out.trim() {
            "playing" => State::Playing,
            "paused" => State::Paused,
            _ => State::Stopped,
        }
    }

    /// Which player a command should go to: one that is playing wins, then one that is
    /// paused, then one that is only open; Spotify before Music on a tie. `None` means
    /// neither is running.
    pub fn choose(spotify: Option<State>, music: Option<State>) -> Option<Player> {
        match (spotify, music) {
            (None, None) => None,
            (Some(_), None) => Some(Player::Spotify),
            (None, Some(_)) => Some(Player::Music),
            (Some(s), Some(m)) => Some(if m > s {
                Player::Music
            } else {
                Player::Spotify
            }),
        }
    }

    /// The title, artist and state from `now_playing_script`. `None` when there is no title
    /// and no artist, or when the answer is not in the expected shape.
    pub fn parse_now_playing(out: &str) -> Option<NowPlaying> {
        let mut parts = out.trim_end_matches(['\n', '\r']).split(SEP);
        let (title, artist, state) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let (title, artist) = (title.trim().to_owned(), artist.trim().to_owned());
        if title.is_empty() && artist.is_empty() {
            return None;
        }
        Some(NowPlaying {
            title,
            artist,
            playing: parse_state(state) == State::Playing,
        })
    }

    /// What a failed `osascript` run means, from its error text. Codes: -1743 the person has
    /// not allowed JARVIS to control the app; -1728 and -600 nothing there to ask (no current
    /// track, or the app closed meanwhile).
    pub fn map_error(stderr: &str) -> MediaError {
        if stderr.contains("-1743") {
            MediaError::NotAllowed
        } else if stderr.contains("-1728") || stderr.contains("-600") {
            MediaError::NothingPlaying
        } else {
            MediaError::Failed
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const COMMANDS: [Command; 7] = [
            Command::Play,
            Command::Pause,
            Command::Next,
            Command::Previous,
            Command::VolumeUp,
            Command::VolumeDown,
            Command::MuteToggle,
        ];

        #[test]
        fn the_player_goes_to_whichever_is_playing_then_paused_then_open() {
            use State::*;
            assert_eq!(choose(None, None), None);
            assert_eq!(choose(Some(Stopped), None), Some(Player::Spotify));
            assert_eq!(choose(None, Some(Playing)), Some(Player::Music));
            assert_eq!(choose(Some(Paused), Some(Playing)), Some(Player::Music));
            assert_eq!(choose(Some(Playing), Some(Paused)), Some(Player::Spotify));
            assert_eq!(choose(Some(Paused), Some(Stopped)), Some(Player::Spotify));
            // A tie goes to Spotify.
            assert_eq!(choose(Some(Playing), Some(Playing)), Some(Player::Spotify));
            assert_eq!(choose(Some(Stopped), Some(Stopped)), Some(Player::Spotify));
        }

        #[test]
        fn every_command_has_exactly_one_kind_of_script() {
            for cmd in COMMANDS {
                let transport = transport_script(Player::Spotify, cmd).is_some();
                let system = system_script(cmd).is_some();
                assert!(
                    transport ^ system,
                    "{cmd:?} must be a transport or a system command"
                );
            }
        }

        #[test]
        fn scripts_name_only_the_two_fixed_apps_and_have_no_leftover_placeholders() {
            let mut all = vec![];
            for p in Player::ALL {
                all.push(state_script(p));
                all.push(now_playing_script(p));
                for cmd in COMMANDS {
                    all.extend(transport_script(p, cmd));
                }
            }
            for cmd in COMMANDS {
                all.extend(system_script(cmd));
            }
            for script in &all {
                assert!(!script.contains('{') && !script.contains('}'), "{script}");
                // Every `application "X"` in a script is one of the two players.
                for part in script.split("application \"").skip(1) {
                    let name = part.split('"').next().unwrap();
                    assert!(name == "Spotify" || name == "Music", "{name} in {script}");
                }
                assert!(!script.contains("do shell script"), "{script}");
                assert!(!script.contains("run script"), "{script}");
                assert!(!script.contains("System Events"), "{script}");
            }
        }

        #[test]
        fn transport_scripts_say_what_they_do() {
            assert_eq!(
                transport_script(Player::Music, Command::Next).unwrap(),
                "tell application \"Music\" to next track"
            );
            assert_eq!(
                transport_script(Player::Spotify, Command::Pause).unwrap(),
                "tell application \"Spotify\" to pause"
            );
            assert_eq!(
                transport_script(Player::Spotify, Command::Previous).unwrap(),
                "tell application \"Spotify\" to previous track"
            );
        }

        #[test]
        fn volume_moves_by_the_step_and_is_clamped_inside_the_script() {
            let up = system_script(Command::VolumeUp).unwrap();
            let down = system_script(Command::VolumeDown).unwrap();
            assert!(up.contains("+ (10)"), "{up}");
            assert!(down.contains("+ (-10)"), "{down}");
            for s in [&up, &down] {
                assert!(s.contains("if v > 100 then set v to 100"));
                assert!(s.contains("if v < 0 then set v to 0"));
            }
            assert!(
                system_script(Command::MuteToggle)
                    .unwrap()
                    .contains("output muted")
            );
        }

        #[test]
        fn player_states_are_read_from_the_word_the_player_gives() {
            assert_eq!(parse_state("playing\n"), State::Playing);
            assert_eq!(parse_state(" paused "), State::Paused);
            assert_eq!(parse_state("stopped"), State::Stopped);
            assert_eq!(parse_state(""), State::Stopped);
            assert_eq!(parse_state("fast forwarding"), State::Stopped);
        }

        #[test]
        fn now_playing_is_read_from_three_fields() {
            let out = format!("Bohemian Rhapsody{SEP}Queen{SEP}playing\n");
            assert_eq!(
                parse_now_playing(&out),
                Some(NowPlaying {
                    title: "Bohemian Rhapsody".into(),
                    artist: "Queen".into(),
                    playing: true
                })
            );
            let paused = format!("Song{SEP}{SEP}paused");
            let n = parse_now_playing(&paused).unwrap();
            assert_eq!(
                (n.title.as_str(), n.artist.as_str(), n.playing),
                ("Song", "", false)
            );
        }

        #[test]
        fn a_bad_or_empty_answer_is_nothing_playing_not_a_made_up_track() {
            assert_eq!(parse_now_playing(""), None);
            assert_eq!(parse_now_playing("no separators"), None);
            assert_eq!(parse_now_playing(&format!("{SEP}{SEP}stopped")), None);
            // Too many fields: a title that somehow carried the separator is not trusted.
            assert_eq!(
                parse_now_playing(&format!("a{SEP}b{SEP}playing{SEP}c")),
                None
            );
        }

        #[test]
        fn osascript_errors_map_to_what_the_person_can_act_on() {
            let denied = "execution error: Not authorized to send Apple events to Spotify. (-1743)";
            assert_eq!(map_error(denied), MediaError::NotAllowed);
            let no_track =
                "execution error: Spotify got an error: Can't get current track. (-1728)";
            assert_eq!(map_error(no_track), MediaError::NothingPlaying);
            assert_eq!(
                map_error("Application isn't running. (-600)"),
                MediaError::NothingPlaying
            );
            assert_eq!(map_error("something else entirely"), MediaError::Failed);
            assert_eq!(map_error(""), MediaError::Failed);
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::applescript::{self as script, Player, State};
    use super::{Command, MediaError, NowPlaying};
    use std::io::Read;
    use std::process::{Command as Process, Stdio};
    use std::time::{Duration, Instant};

    /// Absolute paths: a different `osascript` or `pgrep` earlier on PATH is never run.
    const OSASCRIPT: &str = "/usr/bin/osascript";
    const PGREP: &str = "/usr/bin/pgrep";
    /// Shorter than the tool's own 5 s limit, so a stuck script is killed here and does not
    /// leave a thread behind. The first use can wait on macOS's permission question.
    const SCRIPT_TIMEOUT: Duration = Duration::from_secs(4);

    /// Runs one fixed script and returns what it printed. Blocking.
    fn run(script: &str) -> Result<String, MediaError> {
        run_with(script, &[])
    }

    /// Like `run`, with arguments the script reads from `argv`.
    fn run_with(script: &str, args: &[&str]) -> Result<String, MediaError> {
        let mut child = Process::new(OSASCRIPT)
            .arg("-e")
            .arg(script)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| MediaError::Failed)?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < SCRIPT_TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MediaError::Failed);
                }
            }
        };
        let read = |pipe: Option<&mut dyn Read>| {
            let mut text = String::new();
            if let Some(p) = pipe {
                let _ = p.take(64 * 1024).read_to_string(&mut text);
            }
            text
        };
        let stdout = read(child.stdout.as_mut().map(|p| p as &mut dyn Read));
        let stderr = read(child.stderr.as_mut().map(|p| p as &mut dyn Read));
        if status.success() {
            Ok(stdout)
        } else {
            Err(script::map_error(&stderr))
        }
    }

    /// Is the app's process there? Never launches anything.
    fn running(p: Player) -> Result<bool, MediaError> {
        Process::new(PGREP)
            .args(["-x", p.process()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .map_err(|_| MediaError::Failed)
    }

    fn state_of(p: Player) -> Result<Option<State>, MediaError> {
        if !running(p)? {
            return Ok(None);
        }
        Ok(Some(script::parse_state(&run(&script::state_script(p))?)))
    }

    fn current_player() -> Result<Player, MediaError> {
        let spotify = state_of(Player::Spotify)?;
        let music = state_of(Player::Music)?;
        script::choose(spotify, music).ok_or(MediaError::NothingPlaying)
    }

    pub fn send(cmd: Command) -> Result<(), MediaError> {
        if let Some(system) = script::system_script(cmd) {
            return run(&system).map(|_| ());
        }
        let player = current_player()?;
        let transport = script::transport_script(player, cmd).ok_or(MediaError::Failed)?;
        run(&transport).map(|_| ())
    }

    pub fn play_spotify_uri(uri: &str) -> Result<(), MediaError> {
        if !script::valid_track_uri(uri) {
            return Err(MediaError::Failed);
        }
        if !running(Player::Spotify)? {
            return Err(MediaError::NothingPlaying);
        }
        run_with(script::PLAY_TRACK_SCRIPT, &[uri]).map(|_| ())
    }

    pub fn now_playing() -> Result<Option<NowPlaying>, MediaError> {
        let player = match current_player() {
            Ok(p) => p,
            Err(MediaError::NothingPlaying) => return Ok(None),
            Err(e) => return Err(e),
        };
        match run(&script::now_playing_script(player)) {
            Ok(out) => Ok(script::parse_now_playing(&out)),
            // A player that is open with no track loaded has nothing to report.
            Err(MediaError::NothingPlaying) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::{Command, MediaError, NowPlaying};

    pub fn play_spotify_uri(_: &str) -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
    }

    pub fn send(_: Command) -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
    }

    pub fn now_playing() -> Result<Option<NowPlaying>, MediaError> {
        Err(MediaError::Unsupported)
    }
}

#[cfg(all(test, not(any(windows, target_os = "macos"))))]
mod tests {
    use super::*;

    #[test]
    fn systems_without_an_adapter_say_so_and_do_nothing() {
        assert_eq!(send(Command::Pause), Err(MediaError::Unsupported));
        assert_eq!(send(Command::VolumeUp), Err(MediaError::Unsupported));
        assert_eq!(now_playing(), Err(MediaError::Unsupported));
    }
}

#[cfg(test)]
mod play_track_tests {
    use super::applescript::{PLAY_TRACK_SCRIPT, valid_track_uri};

    #[test]
    fn only_plain_spotify_track_addresses_reach_the_play_script() {
        assert!(valid_track_uri("spotify:track:4u7EnebtmKWzUH433cf5Qv"));
        for bad in [
            "",
            "spotify:track:",
            "spotify:track:4u7EnebtmKWzUH433cf5Q",   // 21
            "spotify:track:4u7EnebtmKWzUH433cf5Qvx", // 23
            "spotify:album:4u7EnebtmKWzUH433cf5Qv",
            "spotify:track:4u7EnebtmKWzUH433cf5Q\"",
            "spotify:track:4u7EnebtmKWzUH433cf5Q ",
            "spotify:track:4u7EnebtmKWzUH433cf5Q\n",
            "https://open.spotify.com/track/4u7EnebtmKWzUH433cf5Qv",
        ] {
            assert!(!valid_track_uri(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_play_script_is_fixed_text_that_reads_its_address_from_argv() {
        assert!(PLAY_TRACK_SCRIPT.contains("item 1 of argv"));
        assert!(!PLAY_TRACK_SCRIPT.contains("spotify:"));
    }

    #[test]
    fn systems_without_the_adapter_refuse_a_play_request() {
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            super::play_spotify_uri("spotify:track:4u7EnebtmKWzUH433cf5Qv"),
            Err(super::MediaError::Unsupported)
        );
        // A bad address is refused everywhere before anything runs.
        assert!(super::play_spotify_uri("nope").is_err());
    }
}
