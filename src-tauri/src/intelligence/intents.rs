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

pub fn route(input: &str) -> Option<Intent> {
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
}
