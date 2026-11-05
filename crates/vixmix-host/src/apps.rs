//! The source panel's app list: every app playing now, then every app a channel names that is
//! not playing, then every other app seen since start ("Seen before"), each once by
//! [`AppRef::matches`].

use vixmix_model::{AppRef, Setup, Source};

use crate::host::AppPresence;

/// How many apps seen since start are remembered beyond the setup's; the oldest go first.
pub(crate) const SEEN_LIMIT: usize = 64;

/// Remembers `app` as seen since start, once, keeping the newest [`SEEN_LIMIT`].
pub(crate) fn remember(seen: &mut Vec<AppRef>, app: &AppRef) {
    if let Some(at) = seen.iter().position(|a| a.matches(app)) {
        seen.remove(at);
    } else if seen.len() == SEEN_LIMIT {
        seen.remove(0);
    }
    seen.push(app.clone());
}

/// The list the source panel shows.
pub(crate) fn presence<'a>(
    playing: impl IntoIterator<Item = &'a AppRef>,
    setup: &Setup,
    seen: &[AppRef],
) -> Vec<AppPresence> {
    let mut out: Vec<AppPresence> = Vec::new();
    let mut add = |app: &AppRef, running: bool| {
        if !out.iter().any(|p| p.app.matches(app)) {
            out.push(AppPresence {
                app: app.clone(),
                running,
            });
        }
    };
    for app in playing {
        add(app, true);
    }
    for channel in &setup.channels {
        if let Source::Apps(apps) = &channel.source {
            for app in apps {
                add(app, false);
            }
        }
    }
    for app in seen.iter().rev() {
        add(app, false);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{ChannelKind, Command, apply};

    fn app(binary: &str) -> AppRef {
        AppRef {
            app_id: None,
            binary: Some(binary.to_string()),
            name: binary.to_string(),
        }
    }

    #[test]
    fn playing_apps_come_first_then_the_setups_then_those_seen_before_each_once() {
        let mut setup = Setup::default();
        let ch = apply(
            &mut setup,
            Command::AddChannel {
                kind: ChannelKind::Apps,
            },
        )
        .unwrap()
        .channel
        .unwrap();
        let _ = apply(
            &mut setup,
            Command::SetSource {
                ch,
                source: Source::Apps(vec![app("spotify"), app("discord")]),
            },
        )
        .unwrap();
        let mut seen = Vec::new();
        for name in ["mpv", "spotify", "firefox", "mpv"] {
            remember(&mut seen, &app(name));
        }
        assert_eq!(seen, vec![app("spotify"), app("firefox"), app("mpv")]);
        let list = presence([&app("spotify"), &app("steam")], &setup, &seen);
        let names: Vec<(&str, bool)> = list
            .iter()
            .map(|p| (p.app.name.as_str(), p.running))
            .collect();
        assert_eq!(
            names,
            [
                ("spotify", true),
                ("steam", true),
                ("discord", false),
                ("mpv", false),
                ("firefox", false),
            ]
        );
    }

    #[test]
    fn the_apps_seen_before_are_bounded() {
        let mut seen = Vec::new();
        for n in 0..SEEN_LIMIT + 10 {
            remember(&mut seen, &app(&format!("app{n}")));
        }
        assert_eq!(seen.len(), SEEN_LIMIT);
        assert_eq!(seen[0], app("app10"), "the oldest went first");
    }
}
