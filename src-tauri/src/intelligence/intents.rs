//! Deterministic local intent router (spec §3.3). No LLM, no network.
//! It only classifies; running the intent goes through the permission gateway (M4).
//! Matching is anchored to the whole utterance, so long free-form questions fall
//! through to the cloud model instead of being guessed at.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    TimeNow,
    DateToday,
    SystemStatus,
    LaunchApp(String),
    Stop,
    Mute,
    Unmute,
    RepeatLast,
    WhatCanYouDo,
    FindFile(String),
    /// "Remember that ...": the text exactly as spoken, for the confirmation card.
    Remember(String),
    /// "Pause", "next song", "volume up": a media-session command, no model needed.
    Music(MusicCmd),
    /// "What's playing?"
    NowPlaying,
    /// "Play Bohemian Rhapsody": the words after "play", for Spotify (music step 2).
    PlayQuery(String),
    /// "Play my workout playlist": one of the person's own playlists, by name.
    PlayPlaylist(String),
    /// "Play my liked songs".
    PlayLiked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicCmd {
    Play,
    Pause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    /// Audio mute (the system key toggles). Plain "mute" still means the microphone.
    Mute,
}

impl MusicCmd {
    /// The `action` of the `music.control` tool.
    pub fn action(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::Mute => "mute",
        }
    }
}

/// Whole-sentence matches only, like the other local intents, so a sentence that merely
/// contains "next" or "pause" goes to the model.
fn music(s: &str) -> Option<Intent> {
    const PAUSE: &[&str] = &[
        "pause",
        "pause the music",
        "pause music",
        "pause the song",
        "pause the track",
        "pause playback",
        "stop the music",
        "stop music",
        "stop the song",
        "stop playback",
    ];
    const PLAY: &[&str] = &[
        "play",
        "resume",
        "resume the music",
        "resume music",
        "resume playing",
        "resume playback",
        "continue playing",
        "continue the music",
        "keep playing",
        "unpause",
        "unpause the music",
        "play music",
        "play the music",
        "play some music",
        "start the music",
        "start music",
    ];
    const NEXT: &[&str] = &[
        "next",
        "next song",
        "next track",
        "next one",
        "skip",
        "skip song",
        "skip track",
        "skip this",
        "skip this song",
        "skip this track",
        "skip it",
        "play next",
        "play the next song",
        "play next song",
        "play the next track",
        "go to the next song",
        "go to the next track",
    ];
    const PREVIOUS: &[&str] = &[
        "previous",
        "previous song",
        "previous track",
        "last song",
        "last track",
        "play previous",
        "play the previous song",
        "play the previous track",
        "play the last song",
        "go back a song",
        "go back one song",
        "go to the previous song",
        "go to the previous track",
    ];
    const VOLUME_UP: &[&str] = &[
        "volume up",
        "turn the volume up",
        "turn volume up",
        "turn up the volume",
        "turn it up",
        "louder",
        "make it louder",
        "increase the volume",
        "raise the volume",
        "turn the music up",
    ];
    const VOLUME_DOWN: &[&str] = &[
        "volume down",
        "turn the volume down",
        "turn volume down",
        "turn down the volume",
        "turn it down",
        "quieter",
        "softer",
        "make it quieter",
        "lower the volume",
        "decrease the volume",
        "reduce the volume",
        "turn the music down",
    ];
    const MUTE: &[&str] = &[
        "mute the music",
        "mute music",
        "mute the volume",
        "mute the sound",
        "mute the audio",
        "unmute the music",
        "unmute the volume",
        "unmute the sound",
        "unmute the audio",
    ];
    const NOW_PLAYING: &[&str] = &[
        "what's playing",
        "whats playing",
        "what is playing",
        "what's playing right now",
        "what is playing right now",
        "what's currently playing",
        "what is currently playing",
        "what song is this",
        "what song is this one",
        "what song is playing",
        "what's this song",
        "whats this song",
        "what is this song",
        "what's the song",
        "what track is this",
        "which song is this",
        "what am i listening to",
        "now playing",
    ];
    let cmd = if one_of(s, PAUSE) {
        MusicCmd::Pause
    } else if one_of(s, PLAY) {
        MusicCmd::Play
    } else if one_of(s, NEXT) {
        MusicCmd::Next
    } else if one_of(s, PREVIOUS) {
        MusicCmd::Previous
    } else if one_of(s, VOLUME_UP) {
        MusicCmd::VolumeUp
    } else if one_of(s, VOLUME_DOWN) {
        MusicCmd::VolumeDown
    } else if one_of(s, MUTE) {
        MusicCmd::Mute
    } else if one_of(s, NOW_PLAYING) {
        return Some(Intent::NowPlaying);
    } else {
        // The person's own things come before a search: "play my liked songs" and "play my workout
        // playlist" are not songs to look for.
        if one_of(s, LIKED) {
            return Some(Intent::PlayLiked);
        }
        if let Some(name) = playlist_name(s) {
            return Some(Intent::PlayPlaylist(name));
        }
        return play_query(s).map(Intent::PlayQuery);
    };
    Some(Intent::Music(cmd))
}

/// "play <words>" with a few words after it: something to look for on Spotify. The bare
/// resume phrases ("play", "play music") were matched before this. A long sentence that
/// merely starts with "play" is left to the model.
const LIKED: &[&str] = &[
    "play my liked songs",
    "play liked songs",
    "play my liked tracks",
    "play my saved songs",
    "play my saved tracks",
    "play my likes",
    "play the songs i liked",
    "play songs i liked",
    "play my favourite songs",
    "play my favorite songs",
];

/// "play my workout playlist", "play the road trip playlist", "play playlist chill",
/// "play my playlist called chill": the name between the words, one to five words, or `None`.
fn playlist_name(s: &str) -> Option<String> {
    const MAX_WORDS: usize = 5;
    let rest = s.strip_prefix("play ")?;
    let name = if let Some(n) = rest
        .strip_suffix(" playlist")
        .or_else(|| rest.strip_suffix(" playlist on spotify"))
    {
        n.strip_prefix("my ")
            .or_else(|| n.strip_prefix("the "))
            .or_else(|| n.strip_prefix("a "))
            .unwrap_or(n)
    } else {
        let n = rest
            .strip_prefix("my playlist called ")
            .or_else(|| rest.strip_prefix("my playlist named "))
            .or_else(|| rest.strip_prefix("the playlist called "))
            .or_else(|| rest.strip_prefix("the playlist named "))
            .or_else(|| rest.strip_prefix("my playlist "))
            .or_else(|| rest.strip_prefix("the playlist "))
            .or_else(|| rest.strip_prefix("playlist "))?;
        n.strip_suffix(" on spotify").unwrap_or(n)
    }
    .trim();
    let words = name.split_whitespace().count();
    // A bare "my" or "the" is not a name, and a long sentence is not either.
    if name.is_empty() || words > MAX_WORDS || matches!(name, "my" | "the" | "a" | "this" | "that")
    {
        return None;
    }
    Some(name.to_owned())
}

fn play_query(s: &str) -> Option<String> {
    const MAX_WORDS: usize = 8;
    const VAGUE: &[&str] = &[
        "it",
        "that",
        "this",
        "something",
        "anything",
        "some music",
        "some songs",
        "songs",
        "a song",
        "the music",
        "music",
        "a game",
        "games",
        "again",
    ];
    let rest = s.strip_prefix("play ")?;
    let rest = rest.strip_prefix("me ").unwrap_or(rest);
    let rest = rest.strip_prefix("something by ").unwrap_or(rest);
    let rest = rest
        .strip_suffix(" on spotify")
        .or_else(|| rest.strip_suffix(" in spotify"))
        .unwrap_or(rest)
        .trim();
    let words = rest.split_whitespace().count();
    if rest.is_empty() || words > MAX_WORDS || VAGUE.contains(&rest) {
        return None;
    }
    Some(rest.to_owned())
}

const FILLER_PREFIXES: &[&str] = &[
    "hey jarvis",
    "ok jarvis",
    "jarvis",
    "please",
    "can you",
    "could you",
    "would you",
];

/// Lowercase, drop punctuation, collapse spaces, strip leading politeness.
fn normalise(input: &str) -> String {
    let cleaned: String = input
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let mut s = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let before = s.len();
        for p in FILLER_PREFIXES {
            if let Some(rest) = s.strip_prefix(p)
                && (rest.is_empty() || rest.starts_with(' '))
            {
                s = rest.trim_start().to_string();
            }
        }
        if s.len() == before {
            break;
        }
    }
    s
}

fn one_of(s: &str, options: &[&str]) -> bool {
    options.contains(&s)
}

/// Longest app/file argument we accept; longer means it is really a sentence.
const MAX_ARG_WORDS: usize = 4;

fn arg_after(s: &str, prefixes: &[&str]) -> Option<String> {
    for p in prefixes {
        if let Some(rest) = s.strip_prefix(p) {
            let rest = rest.trim();
            let rest = rest.strip_prefix("the ").unwrap_or(rest).trim();
            let words = rest.split_whitespace().count();
            if (1..=MAX_ARG_WORDS).contains(&words) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// The text after "remember that", kept exactly as the user wrote it (case and punctuation),
/// because the confirmation card shows it back for approval.
fn remember_text(input: &str) -> Option<String> {
    let t = input.trim();
    for p in [
        "please remember that ",
        "jarvis remember that ",
        "remember that ",
        "remember: ",
    ] {
        if let Some(head) = t.get(..p.len())
            && head.eq_ignore_ascii_case(p)
        {
            let rest = t[p.len()..].trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

pub fn route(input: &str) -> Option<Intent> {
    if let Some(text) = remember_text(input) {
        return Some(Intent::Remember(text));
    }
    let s = normalise(input);
    if s.is_empty() {
        return None;
    }
    if one_of(
        &s,
        &[
            "stop",
            "stop talking",
            "be quiet",
            "quiet",
            "cancel",
            "shut up",
            "go quiet",
            "never mind",
        ],
    ) {
        return Some(Intent::Stop);
    }
    if one_of(
        &s,
        &["mute", "mute yourself", "mute the mic", "mute microphone"],
    ) {
        return Some(Intent::Mute);
    }
    if one_of(
        &s,
        &[
            "unmute",
            "unmute yourself",
            "unmute the mic",
            "unmute microphone",
        ],
    ) {
        return Some(Intent::Unmute);
    }
    if one_of(
        &s,
        &[
            "what time is it",
            "what's the time",
            "whats the time",
            "tell me the time",
            "the time",
            "time",
            "what is the time",
            "what time is it now",
        ],
    ) {
        return Some(Intent::TimeNow);
    }
    if one_of(
        &s,
        &[
            "what's the date",
            "whats the date",
            "what is the date",
            "what's today's date",
            "what day is it",
            "what is today's date",
            "what is today",
            "today's date",
            "what's the date today",
        ],
    ) {
        return Some(Intent::DateToday);
    }
    if one_of(
        &s,
        &[
            "how's my battery",
            "hows my battery",
            "how is my battery",
            "what's my battery",
            "whats my battery",
            "what is my battery",
            "battery",
            "battery status",
            "battery level",
            "how much battery do i have",
            "system status",
            "status",
            "how's the system",
            "how is my system",
        ],
    ) {
        return Some(Intent::SystemStatus);
    }
    if one_of(
        &s,
        &[
            "say that again",
            "repeat that",
            "repeat",
            "say it again",
            "what did you say",
            "come again",
            "sorry what",
        ],
    ) {
        return Some(Intent::RepeatLast);
    }
    if one_of(
        &s,
        &[
            "what can you do",
            "what can you do offline",
            "help",
            "what are your capabilities",
            "what do you do",
        ],
    ) {
        return Some(Intent::WhatCanYouDo);
    }
    if let Some(i) = music(&s) {
        return Some(i);
    }
    if let Some(app) = arg_after(&s, &["open ", "launch ", "start "]) {
        return Some(Intent::LaunchApp(app));
    }
    if let Some(pattern) = arg_after(
        &s,
        &[
            "find my ",
            "find a ",
            "find ",
            "search for my ",
            "search for ",
            "look for my ",
        ],
    ) {
        return Some(Intent::FindFile(pattern));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use Intent::*;

    #[test]
    fn spec_examples() {
        assert_eq!(route("What time is it?"), Some(TimeNow));
        assert_eq!(route("How's my battery?"), Some(SystemStatus));
        assert_eq!(route("Open Spotify"), Some(LaunchApp("spotify".into())));
        assert_eq!(route("Stop"), Some(Stop));
        assert_eq!(route("Go quiet"), Some(Stop));
        assert_eq!(route("Say that again"), Some(RepeatLast));
        assert_eq!(route("What can you do?"), Some(WhatCanYouDo));
        assert_eq!(route("Find my tax PDF"), Some(FindFile("tax pdf".into())));
    }

    #[test]
    fn politeness_and_wake_phrase_are_ignored() {
        assert_eq!(
            route("Hey Jarvis, please open the calculator."),
            Some(LaunchApp("calculator".into()))
        );
        assert_eq!(route("Jarvis, can you mute"), Some(Mute));
        assert_eq!(
            route("hey jarvis hey jarvis what's the date"),
            Some(DateToday)
        );
    }

    #[test]
    fn music_commands_are_matched_as_whole_sentences() {
        assert_eq!(route("Pause"), Some(Music(MusicCmd::Pause)));
        assert_eq!(
            route("Hey Jarvis, please pause the music."),
            Some(Music(MusicCmd::Pause))
        );
        assert_eq!(route("stop the music"), Some(Music(MusicCmd::Pause)));
        assert_eq!(route("Resume"), Some(Music(MusicCmd::Play)));
        assert_eq!(route("play"), Some(Music(MusicCmd::Play)));
        assert_eq!(route("start the music"), Some(Music(MusicCmd::Play)));
        assert_eq!(route("Skip this song"), Some(Music(MusicCmd::Next)));
        assert_eq!(route("next track"), Some(Music(MusicCmd::Next)));
        assert_eq!(
            route("play the previous song"),
            Some(Music(MusicCmd::Previous))
        );
        assert_eq!(route("Turn it up"), Some(Music(MusicCmd::VolumeUp)));
        assert_eq!(route("lower the volume"), Some(Music(MusicCmd::VolumeDown)));
        assert_eq!(route("mute the music"), Some(Music(MusicCmd::Mute)));
        assert_eq!(route("What's playing?"), Some(NowPlaying));
        assert_eq!(route("what song is this"), Some(NowPlaying));
    }

    #[test]
    fn music_words_inside_a_longer_sentence_go_to_the_model() {
        for q in [
            "can the next version of the app skip tests",
            "pause for a moment and think about this",
            "play me something by the Beatles",
            "play Bohemian Rhapsody",
            "what is the next step",
            "why is the volume of trade up",
        ] {
            assert!(!matches!(route(q), Some(Music(_) | NowPlaying)), "{q:?}");
        }
    }

    #[test]
    fn plain_mute_stays_the_microphone_and_the_music_forms_are_separate() {
        assert_eq!(route("mute"), Some(Mute));
        assert_eq!(route("unmute"), Some(Unmute));
        assert_eq!(route("mute the sound"), Some(Music(MusicCmd::Mute)));
        // Opening an app is still opening an app.
        assert_eq!(route("open Spotify"), Some(LaunchApp("spotify".into())));
        assert_eq!(route("start Chrome"), Some(LaunchApp("chrome".into())));
    }

    #[test]
    fn mute_and_unmute_are_distinct() {
        assert_eq!(route("unmute"), Some(Unmute));
        assert_eq!(route("mute yourself"), Some(Mute));
    }

    #[test]
    fn free_form_questions_fall_through_to_the_model() {
        for q in [
            "what's the capital of France",
            "open source licences are confusing, explain the difference between MIT and GPL",
            "stop signs in the UK are what colour",
            "what time zone is Tokyo in",
            "tell me a joke",
            "",
            "   ",
        ] {
            assert_eq!(route(q), None, "{q:?}");
        }
    }

    #[test]
    fn long_arguments_are_not_treated_as_app_names() {
        assert_eq!(route("open the pod bay doors right now hal please"), None);
    }

    #[test]
    fn punctuation_and_case_are_normalised() {
        assert_eq!(route("  WHAT   TIME is it ?!  "), Some(TimeNow));
    }

    #[test]
    fn remember_keeps_the_text_exactly_and_needs_that_or_a_colon() {
        assert_eq!(
            route("Remember that my sister's name is Anaïs."),
            Some(Remember("my sister's name is Anaïs.".into()))
        );
        assert_eq!(
            route("  please REMEMBER THAT I like Tea  "),
            Some(Remember("I like Tea".into()))
        );
        assert_eq!(
            route("remember: call mum"),
            Some(Remember("call mum".into()))
        );
        // Not a request to store anything.
        for q in [
            "remember",
            "remember when we met",
            "remember that",
            "do you remember that",
        ] {
            assert!(!matches!(route(q), Some(Remember(_))), "{q}");
        }
        // The words inside never trigger other intents.
        assert!(matches!(
            route("remember that stop means stop"),
            Some(Remember(_))
        ));
    }

    #[test]
    fn play_followed_by_words_is_a_spotify_request() {
        let q = |t: &str| match route(t) {
            Some(Intent::PlayQuery(q)) => Some(q),
            _ => None,
        };
        assert_eq!(
            q("Play Bohemian Rhapsody"),
            Some("bohemian rhapsody".into())
        );
        assert_eq!(
            q("hey jarvis, play Bohemian Rhapsody by Queen!"),
            Some("bohemian rhapsody by queen".into())
        );
        assert_eq!(
            q("play me something by the Beatles"),
            Some("the beatles".into())
        );
        assert_eq!(q("play lofi beats on Spotify"), Some("lofi beats".into()));
        // The resume phrases stay resume, and vague or long requests go to the model.
        for other in [
            "play",
            "play music",
            "play something",
            "play it",
            "play a game",
            "play the first one of those songs that I like from before please",
            "I want to play chess",
            "can you play something",
        ] {
            assert_eq!(q(other), None, "{other}");
        }
        assert_eq!(route("play"), Some(Intent::Music(MusicCmd::Play)));
        assert_eq!(route("play music"), Some(Intent::Music(MusicCmd::Play)));
    }

    #[test]
    fn my_playlists_and_liked_songs_are_not_searches() {
        let playlist = |t: &str| match route(t) {
            Some(Intent::PlayPlaylist(n)) => Some(n),
            _ => None,
        };
        assert_eq!(playlist("play my workout playlist"), Some("workout".into()));
        assert_eq!(
            playlist("Play the road trip playlist"),
            Some("road trip".into())
        );
        assert_eq!(playlist("play playlist chill"), Some("chill".into()));
        assert_eq!(
            playlist("play my playlist called Sunday morning"),
            Some("sunday morning".into())
        );
        assert_eq!(
            playlist("hey jarvis play chill playlist on spotify"),
            Some("chill".into())
        );
        for liked in [
            "play my liked songs",
            "Play liked songs!",
            "play my saved songs",
        ] {
            assert_eq!(route(liked), Some(Intent::PlayLiked), "{liked}");
        }
        // Not playlists: a search, a bare resume, or a long sentence.
        for other in [
            "play my playlist",
            "play the playlist",
            "play a",
            "play bohemian rhapsody",
            "play the best songs of the nineties and some more playlist",
        ] {
            assert!(playlist(other).is_none(), "{other}");
        }
        assert_eq!(
            route("play bohemian rhapsody"),
            Some(Intent::PlayQuery("bohemian rhapsody".into()))
        );
        assert_eq!(route("play"), Some(Intent::Music(MusicCmd::Play)));
    }
}
