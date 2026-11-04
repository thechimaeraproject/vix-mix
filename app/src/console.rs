//! The console row, laid out as prototype v5's `.console`: the channel strips left to right,
//! the add-channel column after them while there is room for another channel, then — past a
//! wider gap — Monitor and Master. Black shows between the strips. Static for the look gate:
//! it draws what the host says and senses nothing.

use egui::{CornerRadius, Painter, Pos2, Rect, Vec2, pos2, vec2};
use vixmix_host::AppPresence;
use vixmix_model::{MAX_CHANNELS, MixId, Setup, Source};

use crate::meters::Meters;
use crate::strip::{self, ChannelView, MixView};
use crate::theme::{colour, size};

/// What the console draws: the setup, the meters, and which apps are playing.
pub struct ConsoleView<'a> {
    pub setup: &'a Setup,
    pub meters: &'a Meters,
    pub apps: &'a [AppPresence],
}

/// Whether the add-channel column shows: while fewer than eight channels exist.
pub fn has_add_column(channels: usize) -> bool {
    channels < MAX_CHANNELS
}

/// The console's size for `channels` channels: the padding, each channel and the gap after
/// it, the add-channel column and its gap, the margin before the mixes, the two mixes.
pub fn size(channels: usize) -> Vec2 {
    let channel_count = channels.min(MAX_CHANNELS) as f32;
    let add = if has_add_column(channels) {
        size::ADD_COLUMN + size::CONSOLE_GAP
    } else {
        0.0
    };
    let width = size::CONSOLE_PAD
        + channel_count * (size::STRIP + size::CONSOLE_GAP)
        + add
        + size::MIXES_MARGIN
        + 2.0 * size::MIX
        + size::CONSOLE_GAP
        + size::CONSOLE_PAD;
    vec2(width, size::STRIP_HEIGHT + 2.0 * size::CONSOLE_PAD)
}

/// Whether a channel's header shows the live dot: a hardware input always, an app channel
/// while one of its apps is playing (the prototype's `srcText`).
pub fn live(source: &Source, apps: &[AppPresence]) -> bool {
    match source {
        Source::Hardware(_) => true,
        Source::Apps(list) => list
            .iter()
            .any(|app| apps.iter().any(|p| p.running && p.app.matches(app))),
    }
}

/// Draws the console with its top-left corner at `origin`; returns the rect it filled.
pub fn draw(painter: &Painter, origin: Pos2, view: &ConsoleView<'_>) -> Rect {
    let setup = view.setup;
    let rect = Rect::from_min_size(origin, size(setup.channels.len()));
    painter.rect_filled(rect, CornerRadius::ZERO, colour::EDGE);
    let top = rect.top() + size::CONSOLE_PAD;
    let strip_rect =
        |x: f32, width: f32| Rect::from_min_size(pos2(x, top), vec2(width, size::STRIP_HEIGHT));
    let mut x = rect.left() + size::CONSOLE_PAD;
    for (index, channel) in setup.channels.iter().enumerate().take(MAX_CHANNELS) {
        let view = ChannelView {
            channel,
            live: live(&channel.source, view.apps),
            meters: [
                view.meters.send(index, MixId::Monitor),
                view.meters.send(index, MixId::Master),
            ],
        };
        strip::channel(painter, strip_rect(x, size::STRIP), &view);
        x += size::STRIP + size::CONSOLE_GAP;
    }
    if has_add_column(setup.channels.len()) {
        strip::add_column(painter, strip_rect(x, size::ADD_COLUMN));
        x += size::ADD_COLUMN + size::CONSOLE_GAP;
    }
    x += size::MIXES_MARGIN;
    for mix in [&setup.monitor, &setup.master] {
        let view = MixView {
            mix,
            preview: setup.preview_master,
            meters: view.meters.mix(mix.id),
        };
        strip::mix(painter, strip_rect(x, size::MIX), &view);
        x += size::MIX + size::CONSOLE_GAP;
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::example;

    #[test]
    fn the_example_console_is_the_prototypes_size() {
        // Six channels, the add column, two mixes: 900 × 454 points, the prototype's 902 × 490
        // less its 1-point border and its 34-point header.
        assert_eq!(size(6), vec2(900.0, 454.0));
        // Eight channels: no add column (2 + 8 × 106 + 6 + 112 + 2 + 112 + 2).
        assert_eq!(size(8).x, 1084.0);
        assert!(has_add_column(7));
        assert!(!has_add_column(8));
    }

    #[test]
    fn a_channel_is_live_when_it_is_hardware_or_one_of_its_apps_plays() {
        let setup = example::setup();
        let apps: Vec<AppPresence> = example::APPS
            .iter()
            .map(|(id, _, running)| AppPresence {
                app: example::app(id),
                running: *running,
            })
            .collect();
        let lit: Vec<bool> = setup
            .channels
            .iter()
            .map(|c| live(&c.source, &apps))
            .collect();
        assert_eq!(lit, [true, true, true, true, true, true]);
        let nothing_plays: Vec<AppPresence> = apps
            .iter()
            .map(|p| AppPresence {
                running: false,
                ..p.clone()
            })
            .collect();
        assert!(
            live(&setup.channels[0].source, &nothing_plays),
            "Mic is hardware"
        );
        assert!(
            !live(&setup.channels[2].source, &nothing_plays),
            "Music plays nothing"
        );
    }
}
