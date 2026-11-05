//! The window's gestures, each pressed, dragged or typed in a headless frame on
//! `FakeHost`'s example console, each asserting the command it sends (handoff 05's
//! acceptance), and what the setup looks like after.

use egui::{Key, Modifiers, Pos2, vec2};
use vixmix_host::Host;
use vixmix_model::{
    ChannelKind, Command, DeviceRef, MAX_CHANNELS, MixId, Refused, SlotRef, Source, Strip, example,
    taper,
};

use crate::fader::{FINE_DRAG, FINE_KEY_STEP, KEY_STEP, Target};
use crate::popover::{self, MAX_PLUGIN_DIRS, Popover};
use crate::shell::Intent;
use crate::test_support::{Call, FakeWindows, Harness};
use crate::theme::{pop, size};

/// The travel of a channel's fader: its column less 6 at each end.
const TRAVEL: f32 = 210.0 - 2.0 * 6.0;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

/// The one command `sent` holds, or a failure that shows them.
fn one(sent: &[Command]) -> &Command {
    assert_eq!(sent.len(), 1, "{sent:?}");
    &sent[0]
}

fn send_position(cmd: &Command) -> f32 {
    match cmd {
        Command::SetSend { position, .. } | Command::SetMixLevel { position, .. } => *position,
        other => panic!("not a fader's command: {other:?}"),
    }
}

#[test]
fn a_fader_drag_moves_the_send_by_the_pointers_travel() {
    let mut h = Harness::new();
    let mic = h.channel(0);
    let target = Target::Send {
        ch: mic,
        mix: MixId::Monitor,
    };
    let before = h.setup().channels[0].clone();
    let from = h.fader_line(target);
    // Up 20 points in two moves: each move sends where the fader has got to.
    let sent = h.drag(from, &[from - vec2(0.0, 10.0), from - vec2(0.0, 20.0)]);
    assert_eq!(sent.len(), 2, "{sent:?}");
    let expected = before.monitor.position + 20.0 / TRAVEL;
    let Command::SetSend { ch, mix, position } = sent[1] else {
        panic!("{sent:?}")
    };
    assert_eq!((ch, mix), (mic, MixId::Monitor));
    assert!(close(position, expected), "{position} vs {expected}");
    // A press anywhere on the column moves it from where it is, not to the pointer.
    let low = h.fader(target).center_bottom() - vec2(0.0, 20.0);
    let sent = h.drag(low, &[low - vec2(0.0, 10.0)]);
    assert!(close(send_position(one(&sent)), expected + 10.0 / TRAVEL));
    // Mic is not linked: its Master send stays.
    assert_eq!(h.setup().channels[0].master, before.master);
}

#[test]
fn with_shift_a_drag_moves_a_fifth_as_far() {
    let mut h = Harness::new();
    let mic = h.channel(0);
    let target = Target::Send {
        ch: mic,
        mix: MixId::Master,
    };
    let from = h.fader_line(target);
    h.modifiers = Modifiers::SHIFT;
    let sent = h.drag(from, &[from + vec2(0.0, 30.0)]);
    let expected = taper::UNITY - 30.0 / TRAVEL * FINE_DRAG;
    assert_eq!(
        one(&sent),
        &Command::SetSend {
            ch: mic,
            mix: MixId::Master,
            position: send_position(&sent[0])
        }
    );
    assert!(close(send_position(&sent[0]), expected));
}

#[test]
fn on_a_linked_channel_the_other_send_moves_with_it() {
    let mut h = Harness::new();
    let game = h.channel(1);
    assert!(h.setup().channels[1].link);
    let before = h.setup().channels[1].clone();
    let from = h.fader_line(Target::Send {
        ch: game,
        mix: MixId::Monitor,
    });
    let sent = h.drag(from, &[from + vec2(0.0, 19.8)]);
    // The window sends the one fader; the model moves its partner by as much.
    let Command::SetSend { ch, mix, position } = *one(&sent) else {
        panic!("{sent:?}")
    };
    assert_eq!((ch, mix), (game, MixId::Monitor));
    let moved = position - before.monitor.position;
    assert!(close(moved, -0.1), "{moved}");
    let after = &h.setup().channels[1];
    assert!(close(after.master.position, before.master.position + moved));
}

#[test]
fn a_double_click_puts_a_fader_back_at_unity() {
    let mut h = Harness::new();
    let mic = h.channel(0);
    let line = h.fader_line(Target::Send {
        ch: mic,
        mix: MixId::Monitor,
    });
    let sent = h.double_click(line);
    assert_eq!(
        sent.last(),
        Some(&Command::SetSend {
            ch: mic,
            mix: MixId::Monitor,
            position: taper::UNITY
        })
    );
    assert_eq!(h.setup().channels[0].monitor.position, taper::UNITY);
    // A mix's fader too.
    let level = Target::Level(MixId::Monitor);
    let sent = h.drag(
        h.fader_line(level),
        &[h.fader_line(level) + vec2(0.0, 40.0)],
    );
    assert!(send_position(one(&sent)) < taper::UNITY);
    h.wait(1.0);
    let sent = h.double_click(h.fader_line(level));
    assert_eq!(
        sent.last(),
        Some(&Command::SetMixLevel {
            mix: MixId::Monitor,
            position: taper::UNITY
        })
    );
}

#[test]
fn the_keys_step_a_focused_fader() {
    let mut h = Harness::new();
    let synth = h.channel(5);
    let target = Target::Send {
        ch: synth,
        mix: MixId::Monitor,
    };
    let start = h.setup().channels[5].monitor.position;
    // A press gives the fader the keys.
    let _ = h.click(h.fader_line(target));
    let up = h.key(Key::ArrowUp, Modifiers::NONE);
    assert!(close(send_position(one(&up)), start + KEY_STEP));
    let fine = h.key(Key::ArrowDown, Modifiers::SHIFT);
    assert!(close(
        send_position(one(&fine)),
        start + KEY_STEP - FINE_KEY_STEP
    ));
    let right = h.key(Key::ArrowRight, Modifiers::NONE);
    assert!(close(
        send_position(one(&right)),
        start + 2.0 * KEY_STEP - FINE_KEY_STEP
    ));
    assert_eq!(
        h.key(Key::Home, Modifiers::NONE),
        vec![Command::SetSend {
            ch: synth,
            mix: MixId::Monitor,
            position: 0.0
        }]
    );
    // Synth is linked: Home took Master down by as much, clamped at the bottom.
    assert_eq!(h.setup().channels[5].master.position, 0.0);
    assert_eq!(
        h.key(Key::End, Modifiers::NONE),
        vec![Command::SetSend {
            ch: synth,
            mix: MixId::Monitor,
            position: 1.0
        }]
    );
    // The arrows moved the fader, not the focus.
    assert!(h.ctx.memory(|m| m.has_focus(target.id())));
}

#[test]
fn mute_link_and_the_ear_toggle() {
    let mut h = Harness::new();
    let (mic, music) = (h.channel(0), h.channel(2));
    let mute_at = |h: &Harness, target: Target| {
        let column = h.fader(target);
        let row = match target {
            Target::Send { ch, .. } => h.buttons(Strip::Channel(ch)),
            Target::Level(mix) => h.buttons(Strip::Mix(mix)),
        };
        Pos2::new(column.center().x, row.center().y)
    };
    let mic_monitor = Target::Send {
        ch: mic,
        mix: MixId::Monitor,
    };
    assert_eq!(
        h.click(mute_at(&h, mic_monitor)),
        vec![Command::SetMute {
            ch: mic,
            mix: MixId::Monitor,
            mute: true
        }]
    );
    // Music's Master send is muted in the example: its button unmutes it.
    let music_master = Target::Send {
        ch: music,
        mix: MixId::Master,
    };
    assert_eq!(
        h.click(mute_at(&h, music_master)),
        vec![Command::SetMute {
            ch: music,
            mix: MixId::Master,
            mute: false
        }]
    );
    let link = h.buttons(Strip::Channel(mic)).center();
    assert_eq!(
        h.click(link),
        vec![Command::SetLink {
            ch: mic,
            link: true
        }]
    );
    // Monitor's mute is the only button on its row.
    assert_eq!(
        h.click(h.buttons(Strip::Mix(MixId::Monitor)).center()),
        vec![Command::SetMixMute {
            mix: MixId::Monitor,
            mute: true
        }]
    );
    // Master's ear is right of its mute.
    let row = h.buttons(Strip::Mix(MixId::Master));
    let offset = (size::MUTE_BUTTON[0] + size::MIX_BUTTON_GAP) / 2.0;
    assert_eq!(
        h.click(row.center() + vec2(offset, 0.0)),
        vec![Command::SetPreview(true)]
    );
    h.idle(2);
    assert!(h.find("Monitor output now plays Master").is_some());
    assert_eq!(
        h.click(row.center() - vec2(offset, 0.0)),
        vec![Command::SetMixMute {
            mix: MixId::Master,
            mute: true
        }]
    );
}

#[test]
fn mon_and_mst_choose_which_sends_take_the_chain() {
    let mut h = Harness::new();
    let (mic, voice) = (h.channel(0), h.channel(4));
    // Voice chat's MON is off in the example; the last MON drawn is the console's sixth.
    let mon: Vec<Pos2> = h
        .texts
        .iter()
        .filter(|(t, _, _)| t == "MON")
        .map(|(_, r, _)| r.center())
        .collect();
    let mst: Vec<Pos2> = h
        .texts
        .iter()
        .filter(|(t, _, _)| t == "MST")
        .map(|(_, r, _)| r.center())
        .collect();
    assert_eq!((mon.len(), mst.len()), (6, 6));
    assert_eq!(
        h.click(mon[4]),
        vec![Command::SetChainRouting {
            ch: voice,
            mix: MixId::Monitor,
            on: true
        }]
    );
    assert_eq!(
        h.click(mst[0]),
        vec![Command::SetChainRouting {
            ch: mic,
            mix: MixId::Master,
            on: false
        }]
    );
    assert!(h.setup().channels[4].chain_to_monitor);
    assert!(!h.setup().channels[0].chain_to_master);
}

#[test]
fn the_square_bypasses_an_insert() {
    let mut h = Harness::new();
    let music = h.channel(2);
    let ott = h.setup().channels[2].chain[0].instance;
    let slot = h.slot(Strip::Channel(music), 0);
    let square = Pos2::new(
        slot.left() + size::SLOT_PAD_X + size::BYPASS / 2.0,
        slot.center().y,
    );
    assert_eq!(
        h.click(square),
        vec![Command::SetInsertEnabled {
            instance: ott,
            enabled: true
        }]
    );
    assert!(h.setup().channels[2].chain[0].enabled);
}

#[test]
fn an_insert_dragged_within_its_strip_moves_there() {
    let mut h = Harness::new();
    let mic = Strip::Channel(h.channel(0));
    let first = h.slot(mic, 0).center();
    // Into the bottom half of the third insert: it lands after it, at the end.
    let below_third = h.slot(mic, 2).center() + vec2(0.0, 5.0);
    let sent = h.drag(first, &[first + vec2(0.0, 6.0), below_third]);
    assert_eq!(
        sent,
        vec![Command::MoveInsert {
            from: SlotRef {
                strip: mic,
                index: 0
            },
            to: SlotRef {
                strip: mic,
                index: 3
            },
            copy: false
        }]
    );
    let names: Vec<&str> = h.setup().channels[0]
        .chain
        .iter()
        .map(|i| i.plugin.name.as_str())
        .collect();
    assert_eq!(names, ["Pro-C 2", "Pro-L 2", "Noise Suppression for Voice"]);
    // Dropped where it is, it sends nothing; a press that does not move is no drag.
    let second = h.slot(mic, 1).center();
    assert!(h.drag(second, &[second + vec2(0.0, 8.0)]).is_empty());
    assert!(h.drag(second, &[second + vec2(0.0, 2.0)]).is_empty());
}

#[test]
fn an_insert_dragged_to_another_strip_moves_or_with_ctrl_copies() {
    let mut h = Harness::new();
    let (mic, game) = (Strip::Channel(h.channel(0)), Strip::Channel(h.channel(1)));
    let compressor = h.slot(mic, 1).center();
    let instance = h.setup().channels[0].chain[1].instance;
    // Anywhere over Game's strip lands in its well; it has no inserts, so first.
    let over_game = h.strip(game).center();
    let sent = h.drag(compressor, &[compressor + vec2(10.0, 0.0), over_game]);
    assert_eq!(
        sent,
        vec![Command::MoveInsert {
            from: SlotRef {
                strip: mic,
                index: 1
            },
            to: SlotRef {
                strip: game,
                index: 0
            },
            copy: false
        }]
    );
    assert_eq!(
        h.setup().channels[1].chain[0].instance,
        instance,
        "the same instance moves"
    );
    h.idle(2);
    assert!(h.find("Moved Pro-C 2 to Game").is_some(), "{:?}", h.drawn());

    // With Ctrl, a copy: the original stays.
    let limiter = h.slot(mic, 1).center();
    h.modifiers = Modifiers::CTRL;
    let sent = h.drag(limiter, &[limiter + vec2(10.0, 0.0), over_game]);
    h.modifiers = Modifiers::NONE;
    assert_eq!(
        sent,
        vec![Command::MoveInsert {
            from: SlotRef {
                strip: mic,
                index: 1
            },
            to: SlotRef {
                strip: game,
                index: 1
            },
            copy: true
        }]
    );
    assert_eq!(h.setup().channels[0].chain.len(), 2);
    assert_eq!(h.setup().channels[1].chain.len(), 2);
    h.idle(1);
    assert!(h.find("Copied Pro-L 2 to Game").is_some());
}

#[test]
fn a_drag_shows_its_ghost_and_escape_cancels_it() {
    let mut h = Harness::new();
    let mic = Strip::Channel(h.channel(0));
    let from = h.slot(mic, 0).center();
    let over = h.strip(Strip::Mix(MixId::Master)).center();
    h.modifiers = Modifiers::CTRL;
    let _ = h.pass(vec![
        egui::Event::PointerMoved(from),
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::CTRL,
        },
    ]);
    // The ghost's first frame sizes it unseen.
    let _ = h.hover(over);
    let _ = h.hover(over);
    assert!(h.find("+ Copy").is_some(), "{:?}", h.drawn());
    assert!(h.shell.gestures.insert.as_ref().is_some_and(|d| d.active));
    assert!(h.key(Key::Escape, Modifiers::NONE).is_empty());
    assert_eq!(h.shell.gestures.insert, None);
    let sent = h.pass(vec![egui::Event::PointerButton {
        pos: over,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::CTRL,
    }]);
    assert!(sent.is_empty(), "a cancelled drag drops nothing: {sent:?}");
}

#[test]
fn the_browser_searches_and_enter_adds_the_first_hit() {
    let mut h = Harness::new();
    let game = Strip::Channel(h.channel(1));
    let add = h.slot(game, 0).center();
    let _ = h.click(add);
    assert!(matches!(h.shell.popover, Some(Popover::Browser(_))));
    h.idle(2);
    assert!(
        h.find("Search 42 plugins by name, maker or type").is_some(),
        "{:?}",
        h.drawn()
    );
    assert!(h.find("Adds to Game, slot 1").is_some(), "{:?}", h.drawn());
    // The field has the focus: typing searches; Down picks the second hit; Enter adds it.
    let _ = h.type_text("tdr");
    let _ = h.key(Key::ArrowDown, Modifiers::NONE);
    let sent = h.key(Key::Enter, Modifiers::NONE);
    // "tdr" finds TDR Kotelnikov, then TDR Nova.
    assert_eq!(
        sent,
        vec![Command::AddInsert {
            strip: game,
            plugin: example::plugin("TDR Nova"),
            at: 0
        }]
    );
    assert_eq!(h.shell.popover, None, "adding closes the browser");
    assert_eq!(h.setup().channels[1].chain[0].plugin.name, "TDR Nova");
    h.idle(2);
    assert!(h.find("Added TDR Nova to Game").is_some());
}

#[test]
fn the_browser_nav_filters_and_a_star_toggles_a_favourite() {
    let mut h = Harness::new();
    let monitor = Strip::Mix(MixId::Monitor);
    let _ = h.click(h.slot(monitor, 1).center());
    h.idle(2);
    let _ = h.click(h.text("Favorites"));
    h.idle(1);
    for name in ["Noise Suppression for Voice", "OTT", "Pro-L 2", "Pro-Q 3"] {
        assert!(h.find(name).is_some(), "{name} in {:?}", h.drawn());
    }
    assert!(h.find("Gullfoss").is_none());
    // The star left of Pro-Q 3 unstars it, and Favorites forgets it.
    let count = |h: &Harness| h.drawn().iter().filter(|t| **t == "Pro-Q 3").count();
    assert_eq!(count(&h), 3, "Monitor's, Master's and the list's");
    let name = h.text_rect("Pro-Q 3");
    let star = Pos2::new(
        name.left() - pop::PLUGIN_GAP - pop::STAR_COLUMN / 2.0,
        name.center().y,
    );
    let from = h.host.calls.len();
    let _ = h.click(star);
    let pro_q = example::plugin("Pro-Q 3").id;
    assert!(
        h.intents
            .iter()
            .any(|i| matches!(i, Intent::Favorite { id, on: false } if *id == pro_q)),
        "{:?}",
        h.intents
    );
    // The host keeps it (in `plugins.ron`, for the real one).
    assert_eq!(h.host.calls[from..], [Call::Favorite(pro_q.clone(), false)]);
    assert!(!h.host.catalog().favorites.contains(&pro_q));
    h.idle(1);
    assert_eq!(count(&h), 2, "{:?}", h.drawn());
    assert!(h.find("3").is_some(), "Favorites counts three");
    // A press on a row adds it at the end of Monitor's chain.
    let sent = h.click(h.text("OTT"));
    assert_eq!(
        sent,
        vec![Command::AddInsert {
            strip: monitor,
            plugin: example::plugin("OTT"),
            at: 1
        }]
    );
}

#[test]
fn an_app_ticked_in_one_channel_leaves_another() {
    let mut h = Harness::new();
    let (music, browser) = (h.channel(2), h.channel(3));
    let _ = h.click(h.head(Strip::Channel(browser)).center());
    h.idle(2);
    for heading in ["PLAYING NOW", "SEEN BEFORE"] {
        assert!(h.find(heading).is_some(), "{heading} in {:?}", h.drawn());
    }
    assert!(h.find("in Music").is_some(), "{:?}", h.drawn());
    let sent = h.click(h.text("Spotify"));
    assert_eq!(
        sent,
        vec![Command::SetSource {
            ch: browser,
            source: Source::Apps(vec![
                example::app("firefox"),
                example::app("chromium"),
                example::app("spotify")
            ])
        }]
    );
    let Source::Apps(music_apps) = &h.setup().channel(music).unwrap().source else {
        panic!("Music is an app channel")
    };
    assert!(!music_apps.contains(&example::app("spotify")));
    // Unticking takes it out; the search narrows the list.
    h.idle(1);
    let sent = h.click(h.text("Firefox"));
    assert_eq!(
        sent,
        vec![Command::SetSource {
            ch: browser,
            source: Source::Apps(vec![example::app("chromium"), example::app("spotify")])
        }]
    );
    let _ = h.click(h.text("Find an app"));
    let _ = h.type_text("thun");
    h.idle(1);
    assert!(h.find("Thunderbird").is_some());
    assert!(h.find("Discord").is_none());
}

#[test]
fn removing_an_insert_shows_undo_and_undo_puts_it_back() {
    let mut h = Harness::new();
    let mic = Strip::Channel(h.channel(0));
    let removed = h.setup().channels[0].chain[1].clone();
    let slot = h.slot(mic, 1);
    // Under the pointer, the slot shows its gear and bin; the bin is the right one.
    let _ = h.hover(slot.center());
    let bin = Pos2::new(
        slot.right() - size::HOVER_INSET[0] - size::HOVER_BUTTON / 2.0,
        slot.top() + size::HOVER_INSET[1] + size::HOVER_BUTTON / 2.0,
    );
    let from = h.host.calls.len();
    let sent = h.click(bin);
    assert_eq!(
        sent,
        vec![Command::RemoveInsert {
            instance: removed.instance
        }]
    );
    // Its editor closed through the host before the insert went.
    assert_eq!(
        h.host.calls[from..],
        [
            Call::CloseEditor(removed.instance),
            Call::Apply(Command::RemoveInsert {
                instance: removed.instance
            })
        ]
    );
    h.idle(2);
    assert!(
        h.find("Removed Pro-C 2 from Mic").is_some(),
        "{:?}",
        h.drawn()
    );
    // Undo applies the one command the host handed out with the removal: the same insert,
    // instance and all, back in its slot.
    let sent = h.click(h.text("Undo"));
    assert_eq!(
        sent,
        vec![Command::RestoreInsert {
            to: SlotRef {
                strip: mic,
                index: 1
            },
            insert: removed.clone()
        }]
    );
    assert_eq!(h.setup().channels[0].chain[1], removed);
    h.idle(1);
    assert!(h.find("Undo").is_none(), "Undo hides the toast");
}

#[test]
fn a_bypassed_insert_comes_back_bypassed_and_undo_dies_with_its_toast() {
    let mut h = Harness::new();
    let music = Strip::Channel(h.channel(2));
    let ott = h.setup().channels[2].chain[0].instance;
    // The slot's own keys: Delete removes it.
    let _ = h.click(h.slot(music, 0).center());
    let sent = h.key(Key::Delete, Modifiers::NONE);
    assert_eq!(sent, vec![Command::RemoveInsert { instance: ott }]);
    h.idle(2);
    let undo = h.text("Undo");
    let sent = h.click(undo);
    let Command::RestoreInsert { insert, .. } = one(&sent) else {
        panic!("{sent:?}")
    };
    assert!(!insert.enabled);
    assert_eq!(h.setup().channels[2].chain[0].instance, ott);
    assert!(
        !h.setup().channels[2].chain[0].enabled,
        "bypassed as it was"
    );
    // Gone after five seconds, and its Undo with it.
    let _ = h.click(h.slot(music, 0).center());
    let _ = h.key(Key::Delete, Modifiers::NONE);
    h.idle(1);
    let undo = h.text("Undo");
    h.wait(pop::TOAST_WITH_ACTION_S + 1.0);
    assert!(h.click(undo).is_empty());
    assert!(h.setup().channels[2].chain.is_empty());
}

#[test]
fn removing_a_channel_shows_undo_and_undo_brings_it_back() {
    let mut h = Harness::new();
    let music = h.setup().channels[2].clone();
    let ott = music.chain[0].instance;
    let _ = h.click(h.head(Strip::Channel(music.id)).center());
    h.idle(2);
    let from = h.host.calls.len();
    let sent = h.click(h.text("Remove channel"));
    assert_eq!(sent, vec![Command::RemoveChannel(music.id)]);
    assert_eq!(h.host.calls[from], Call::CloseEditor(ott));
    assert_eq!(h.shell.popover, None);
    assert_eq!(h.setup().channels.len(), 5);
    h.idle(2);
    // The host's words, and its Undo: the channel back at its place, ids, chain and all.
    assert!(h.find("Removed channel Music").is_some(), "{:?}", h.drawn());
    let sent = h.click(h.text("Undo"));
    assert_eq!(
        sent,
        vec![Command::RestoreChannel {
            at: 2,
            channel: music.clone()
        }]
    );
    assert_eq!(h.setup().channels[2], music);
    assert_eq!(h.setup().channels.len(), 6);
}

#[test]
fn an_undo_the_host_refuses_says_why_on_the_toast_line() {
    let mut h = Harness::new();
    let music = h.setup().channels[2].clone();
    let _ = h.click(h.head(Strip::Channel(music.id)).center());
    h.idle(2);
    let _ = h.click(h.text("Remove channel"));
    h.idle(2);
    let undo = h.text("Undo");
    // Three channels added elsewhere while the toast shows: the console is full again.
    for _ in 0..3 {
        let _ = h.host.inner.apply(Command::AddChannel {
            kind: ChannelKind::Apps,
        });
    }
    h.idle(1);
    let sent = h.click(undo);
    assert!(
        matches!(one(&sent), Command::RestoreChannel { .. }),
        "{sent:?}"
    );
    h.idle(1);
    let refused = Refused::TooManyChannels { max: MAX_CHANNELS }.to_string();
    assert!(h.find(&refused).is_some(), "{:?}", h.drawn());
    assert_eq!(h.setup().channels.len(), MAX_CHANNELS);
}

#[test]
fn the_output_panel_picks_a_device_and_master_can_have_none() {
    let mut h = Harness::new();
    let _ = h.click(h.head(Strip::Mix(MixId::Master)).center());
    h.idle(2);
    assert!(h.find("Apps can also record Master as an input.").is_some());
    assert_eq!(
        h.click(h.text("No device")),
        vec![Command::SetMixDevice {
            mix: MixId::Master,
            device: None
        }]
    );
    h.idle(1);
    let speakers = h.text("Built-in speakers");
    let sent = h.click(speakers);
    let Command::SetMixDevice {
        mix: MixId::Master,
        device: Some(DeviceRef { description, .. }),
    } = one(&sent)
    else {
        panic!("{sent:?}")
    };
    assert_eq!(description, "Built-in speakers");
    let _ = h.click(h.text("Done"));
    assert_eq!(h.shell.popover, None);
    // Monitor offers no "No device".
    let _ = h.click(h.head(Strip::Mix(MixId::Monitor)).center());
    h.idle(2);
    assert!(h.find("No device").is_none(), "{:?}", h.drawn());
}

#[test]
fn a_hardware_channel_picks_its_input() {
    let mut h = Harness::new();
    let synth = h.channel(5);
    let _ = h.click(h.head(Strip::Channel(synth)).center());
    h.idle(2);
    let sent = h.click(h.text("C920 webcam mic"));
    assert_eq!(
        sent,
        vec![Command::SetSource {
            ch: synth,
            source: Source::Hardware(Some(example::device("C920 webcam mic", true)))
        }]
    );
}

#[test]
fn the_name_field_renames_once_when_the_panel_closes() {
    let mut h = Harness::new();
    let mic = h.channel(0);
    let _ = h.click(h.head(Strip::Channel(mic)).center());
    h.idle(2);
    let field = h.text("Mic");
    let _ = h.click(field);
    let _ = h.key(Key::A, Modifiers::COMMAND);
    let typed = h.type_text("Main mic");
    assert!(typed.is_empty(), "nothing is sent while typing: {typed:?}");
    h.idle(1);
    // The header follows the field.
    assert!(h.texts.iter().filter(|(t, _, _)| t == "Main mic").count() >= 2);
    let sent = h.click(h.text("Done"));
    assert_eq!(
        sent,
        vec![Command::Rename {
            ch: mic,
            name: "Main mic".to_string()
        }]
    );
}

#[test]
fn a_press_outside_or_escape_closes_a_popover() {
    let mut h = Harness::new();
    let master = Strip::Mix(MixId::Master);
    let _ = h.click(h.head(master).center());
    h.idle(2);
    assert!(h.shell.popover.is_some());
    let _ = h.key(Key::Escape, Modifiers::NONE);
    assert_eq!(h.shell.popover, None);
    // Its header has the focus back.
    assert!(h.ctx.memory(|m| m.has_focus(crate::strip::head_id(master))));
    let _ = h.click(h.head(master).center());
    h.idle(2);
    // A press on the console outside it closes it, and the press still lands.
    let mic = h.channel(0);
    let line = h.fader_line(Target::Send {
        ch: mic,
        mix: MixId::Master,
    });
    let _ = h.click(line);
    assert_eq!(h.shell.popover, None);
    // Its own header toggles it.
    let _ = h.click(h.head(master).center());
    h.idle(2);
    let _ = h.click(h.head(master).center());
    assert_eq!(h.shell.popover, None);
}

#[test]
fn the_add_column_adds_a_channel_and_opens_its_panel_to_name_it() {
    let mut h = Harness::new();
    let _ = h.click(h.add_column().center());
    assert_eq!(h.shell.popover, Some(Popover::AddChannel));
    h.idle(2);
    let sent = h.click(h.text("Hardware input"));
    assert_eq!(
        sent,
        vec![Command::AddChannel {
            kind: ChannelKind::Hardware
        }]
    );
    let added = h.setup().channels.last().unwrap().id;
    let Some(Popover::Source(panel)) = &h.shell.popover else {
        panic!("{:?}", h.shell.popover)
    };
    assert_eq!(panel.ch, added);
    assert!(panel.focus_name);
    h.idle(2);
    // The name is selected: typing replaces it.
    let _ = h.type_text("Index mic");
    let _ = h.key(Key::Enter, Modifiers::NONE);
    h.idle(1);
    assert_eq!(h.setup().channels.last().unwrap().name, "Index mic");
}

#[test]
fn the_gear_or_a_double_click_asks_for_the_insert_s_editor() {
    let mut h = Harness::new();
    let master = Strip::Mix(MixId::Master);
    let eq = h.setup().master.chain[0].instance;
    let slot = h.slot(master, 0);
    let _ = h.hover(slot.center());
    let gear = Pos2::new(
        slot.right()
            - size::HOVER_INSET[0]
            - size::HOVER_BUTTON
            - size::HOVER_GAP
            - size::HOVER_BUTTON / 2.0,
        slot.top() + size::HOVER_INSET[1] + size::HOVER_BUTTON / 2.0,
    );
    let _ = h.click(gear);
    assert_eq!(h.intents, vec![Intent::OpenEditor(eq)]);
    assert_eq!(h.shell.to_open, vec![eq]);
    h.shell.to_open.clear();
    h.wait(1.0);
    let _ = h.double_click(Pos2::new(slot.center().x - 20.0, slot.center().y));
    assert_eq!(h.shell.to_open, vec![eq]);
}

#[test]
fn an_editor_the_host_refuses_says_why_on_the_toast_line_and_keeps_no_window() {
    let mut h = Harness::new();
    let eq = h.setup().master.chain[0].instance;
    let _ = h.double_click(h.slot(Strip::Mix(MixId::Master), 0).center());
    assert_eq!(h.shell.to_open, vec![eq]);
    // What the window does with the queue once the frame is done, with windows of its own.
    let windows = FakeWindows::default();
    h.shell.open_editors(&windows, &mut h.host, &mut h.editors);
    assert!(h.shell.to_open.is_empty());
    assert!(h.editors.is_empty(), "no window kept for a refusal");
    h.idle(1);
    let toast = "Pro-Q 3 \u{b7} Master \u{b7} slot 1: FakeHost has example data and no plugin instances: run with the real host to open a plugin's editor";
    assert!(h.find(toast).is_some(), "{:?}", h.drawn());
}

#[test]
fn a_panel_opens_with_the_focus_on_its_checked_radio_and_the_arrows_pick() {
    let mut h = Harness::new();
    // Synth plays Scarlett In 2; the webcam is the input after it, and the last.
    let synth = h.channel(5);
    let _ = h.click(h.head(Strip::Channel(synth)).center());
    h.idle(2);
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    let input = |i: usize| example::device(example::INPUTS[i], true);
    assert_eq!(focused(&h), Some(popover::input_id(&input(2))));
    // Down moves to the webcam and picks it, as a browser's radio group does.
    let sent = h.key(Key::ArrowDown, Modifiers::NONE);
    let pick = |i: usize| Command::SetSource {
        ch: synth,
        source: Source::Hardware(Some(input(i))),
    };
    assert_eq!(sent, vec![pick(3)]);
    assert_eq!(focused(&h), Some(popover::input_id(&input(3))));
    // Past the last row, round to the first; the focus stays in the panel.
    let sent = h.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sent, vec![pick(0)]);
    assert_eq!(focused(&h), Some(popover::input_id(&input(0))));
    // Space on the checked row picks nothing new.
    assert!(h.key(Key::Space, Modifiers::NONE).is_empty());
    let _ = h.key(Key::Escape, Modifiers::NONE);

    // Master's output panel: its output checked, "No device" first, Phones between.
    let _ = h.click(h.head(Strip::Mix(MixId::Master)).center());
    h.idle(2);
    let output = |i: usize| example::device(example::OUTPUTS[i], false);
    assert_eq!(focused(&h), Some(popover::output_id(Some(&output(1)))));
    let sent = h.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(
        sent,
        vec![Command::SetMixDevice {
            mix: MixId::Master,
            device: Some(output(0))
        }]
    );
    let sent = h.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(
        sent,
        vec![Command::SetMixDevice {
            mix: MixId::Master,
            device: None
        }]
    );
    assert_eq!(focused(&h), Some(popover::output_id(None)));
}

#[test]
fn the_preferences_hold_at_most_sixteen_plugin_folders() {
    let mut h = Harness::new();
    let full: Vec<std::path::PathBuf> = (0..MAX_PLUGIN_DIRS)
        .map(|i| std::path::PathBuf::from(format!("D:/VST3/{i}")))
        .collect();
    let prefs = vixmix_model::Prefs {
        plugin_dirs: full.clone(),
        ..h.setup().prefs.clone()
    };
    assert!(h.host.apply(Command::SetPrefs(prefs)).unwrap().changed());
    let menu = Pos2::new(
        size::HEADER_PAD_X + size::MENU_BUTTON[0] / 2.0,
        size::HEADER / 2.0,
    );
    let _ = h.click(menu);
    h.idle(2);
    let _ = h.click(h.text("Preferences"));
    h.idle(2);
    let _ = h.click(h.text("Another folder to scan"));
    let _ = h.type_text("E:/More");
    let sent = h.key(Key::Enter, Modifiers::NONE);
    assert!(sent.is_empty(), "{sent:?}");
    h.idle(1);
    let toast = "16 plugin folders is the most there can be: remove one, then add E:/More";
    assert!(h.find(toast).is_some(), "{:?}", h.drawn());
    assert_eq!(h.setup().prefs.plugin_dirs, full);
    // The field keeps the folder, to add once there is room.
    let Some(Popover::Prefs(panel)) = &h.shell.popover else {
        panic!("{:?}", h.shell.popover)
    };
    assert_eq!(panel.folder, "E:/More");
}

#[test]
fn a_refusal_reaches_the_toast_line_with_its_fix() {
    let mut h = Harness::new();
    let mic = h.channel(0);
    h.shell.dispatch(
        &mut h.host,
        &mut h.editors,
        vec![Intent::Apply(Command::SetSend {
            ch: mic,
            mix: MixId::Monitor,
            position: 2.0,
        })],
    );
    h.idle(2);
    assert!(
        h.drawn().iter().any(|t| t.contains("clamp it to 0..=1")),
        "{:?}",
        h.drawn()
    );
}

#[test]
fn the_window_keeps_room_for_the_header_over_a_narrow_console() {
    let mut h = Harness::new();
    // A first start: one channel, a console 370 points wide.
    while h.setup().channels.len() > 1 {
        let ch = h.channel(1);
        let _ = h
            .host
            .inner
            .apply(Command::RemoveChannel(ch))
            .expect("removed");
    }
    h.idle(1);
    let console = crate::console::size(1);
    assert_eq!(console.x, 370.0);
    let needed = h.shell.content;
    assert!(needed.width() > console.x, "{needed:?}");
    assert_eq!(needed.bottom(), size::HEADER + console.y);
    // At that width the display clears the title.
    h.screen = egui::Rect::from_min_size(Pos2::ZERO, needed.size());
    h.idle(2);
    let title = h.text_rect(crate::ui::TITLE);
    let rate = h.text_rect("48");
    assert!(title.right() < rate.left(), "{title:?} against {rate:?}");
}

#[test]
fn a_long_chain_scrolls_in_its_well() {
    let mut h = Harness::new();
    let game = Strip::Channel(h.channel(1));
    // Nine inserts: more than the well's 128 points hold.
    for at in 0..9 {
        let _ = h
            .host
            .inner
            .apply(Command::AddInsert {
                strip: game,
                plugin: example::plugin("OTT"),
                at,
            })
            .expect("added");
    }
    h.idle(1);
    let well = crate::strip::Rows::of(h.strip(game)).fx;
    let add_row = |h: &Harness| {
        h.texts
            .iter()
            .find(|(t, r, _)| t == "Add plugin" && well.x_range().contains(r.center().x))
            .map(|(_, r, _)| r.center().y)
            .expect("Game's add row")
    };
    let before = add_row(&h);
    assert!(before > well.bottom(), "below the well until scrolled");
    let _ = h.pass(vec![
        egui::Event::PointerMoved(well.center()),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -200.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.idle(2);
    let after = add_row(&h);
    assert!(
        after < before && after < well.bottom(),
        "{before} -> {after}"
    );
    // And it adds there.
    let sent = h.click(Pos2::new(well.center().x, after));
    h.idle(2);
    assert!(sent.is_empty(), "the browser opens; nothing is sent yet");
    assert!(matches!(h.shell.popover, Some(Popover::Browser(_))));
}

#[test]
fn the_preferences_ask_for_a_quantum_and_take_plugin_folders() {
    let mut h = Harness::new();
    let menu = Pos2::new(
        size::HEADER_PAD_X + size::MENU_BUTTON[0] / 2.0,
        size::HEADER / 2.0,
    );
    let _ = h.click(menu);
    assert_eq!(h.shell.popover, Some(Popover::Menu));
    h.idle(2);
    // Rescan plugins asks the host and closes the menu; when the host has nothing of its
    // own to say about it, the toast line says what the catalog holds now.
    let from = h.host.calls.len();
    assert!(h.click(h.text("Rescan plugins")).is_empty());
    assert_eq!(h.host.calls[from..], [Call::Rescan]);
    assert_eq!(h.shell.popover, None);
    h.idle(1);
    let plugins = h.host.catalog().plugins.len();
    let said = format!("Rescanned the plugin folders: {plugins} plugins");
    assert!(h.find(&said).is_some(), "{:?}", h.drawn());
    // A host that warned about the rescan keeps its warning on the toast line.
    h.host.news = vec![
        vixmix_host::HostEvent::Toast("Pro-C 2 is missing: rescan plugins".to_string()),
        vixmix_host::HostEvent::PluginsRescanned,
    ];
    h.idle(1);
    assert!(h.find("Pro-C 2 is missing: rescan plugins").is_some());
    let _ = h.click(menu);
    h.idle(2);
    let _ = h.click(h.text("Preferences"));
    h.idle(2);
    let sent = h.click(h.text("512 samples"));
    let Command::SetPrefs(prefs) = one(&sent) else {
        panic!("{sent:?}")
    };
    assert_eq!(prefs.quantum, 512);
    let _ = h.click(h.text("Another folder to scan"));
    let _ = h.type_text("D:/VST3");
    let sent = h.key(Key::Enter, Modifiers::NONE);
    let Command::SetPrefs(prefs) = one(&sent) else {
        panic!("{sent:?}")
    };
    assert_eq!(prefs.plugin_dirs, [std::path::PathBuf::from("D:/VST3")]);
    assert_eq!(prefs.quantum, 512, "the quantum stays");
}

#[test]
fn an_app_search_that_finds_nothing_says_so() {
    let mut h = Harness::new();
    let game = h.channel(1);
    let _ = h.click(h.head(Strip::Channel(game)).center());
    h.idle(2);
    let _ = h.click(h.text("Find an app"));
    let _ = h.type_text("zzz");
    h.idle(1);
    assert!(h.find("No app matches.").is_some(), "{:?}", h.drawn());
    assert!(h.find("PLAYING NOW").is_none());
}

#[test]
fn an_insert_playing_as_a_pass_through_is_marked_and_says_why() {
    let mut h = Harness::new();
    let mic = Strip::Channel(h.channel(0));
    let insert = h.setup().channels[0].chain[1].clone();
    let message = "Pro-C 2 is not installed on this machine: install it and rescan plugins, or remove the insert";
    h.host.problems = vec![vixmix_host::InsertProblem {
        strip: mic,
        instance: insert.instance,
        plugin_id: insert.plugin.id.clone(),
        message: message.to_string(),
    }];
    h.idle(1);
    let ink = |h: &Harness, text: &str| {
        h.texts
            .iter()
            .find(|(t, _, _)| t == text)
            .map(|(_, _, colour)| *colour)
    };
    assert_eq!(ink(&h, "Pro-C 2"), Some(crate::theme::colour::MUTE));
    assert_eq!(
        ink(&h, "Noise Suppression for Voice"),
        Some(crate::theme::colour::TEXT),
        "only the failing slot is marked"
    );
    // Under the pointer it says why, and what to do.
    let _ = h.hover(h.slot(mic, 1).center());
    h.wait(1.0);
    let _ = h.hover(h.slot(mic, 1).center());
    assert!(h.find(message).is_some(), "{:?}", h.drawn());
    // Back to normal once the host has no problem with it.
    h.host.problems.clear();
    h.idle(1);
    assert_eq!(ink(&h, "Pro-C 2"), Some(crate::theme::colour::TEXT));
}

#[test]
fn the_banner_says_when_the_mixer_is_idle_or_the_daemon_is_gone() {
    let mut h = Harness::new();
    let idle = "Nothing records Master and Monitor has no device: pick Monitor's output";
    h.host.news = vec![vixmix_host::HostEvent::Backend(
        vixmix_host::BackendState::Idle {
            reason: idle.to_string(),
        },
    )];
    h.idle(1);
    assert!(h.find(idle).is_some(), "{:?}", h.drawn());
    h.host.news = vec![vixmix_host::HostEvent::Backend(
        vixmix_host::BackendState::Disconnected,
    )];
    h.idle(1);
    assert!(h.find(idle).is_none());
    assert!(
        h.find(vixmix_host::BackendState::DISCONNECTED).is_some(),
        "{:?}",
        h.drawn()
    );
    h.host.news = vec![vixmix_host::HostEvent::Backend(
        vixmix_host::BackendState::Running {
            rate: 48_000,
            quantum: 256,
        },
    )];
    h.idle(1);
    assert!(h.find(vixmix_host::BackendState::DISCONNECTED).is_none());
}

#[test]
fn a_host_message_after_a_quiet_spell_shows_for_its_whole_time() {
    let mut h = Harness::new();
    // Twenty seconds with no frame (the window draws nothing while the mixer is idle), then
    // the host says something.
    h.quiet(20.0);
    let said = "the plugin list could not be kept in /tmp/vix-mix: check the directory is writable";
    h.host.news = vec![vixmix_host::HostEvent::Toast(said.to_string())];
    h.idle(1);
    assert!(h.find(said).is_some(), "{:?}", h.drawn());
    // Its eight seconds count from when it came, not from the last frame before it.
    h.wait(7.0);
    assert!(
        h.find(said).is_some(),
        "still there 7 s on: {:?}",
        h.drawn()
    );
    h.wait(1.5);
    h.idle(1);
    assert!(
        h.find(said).is_none(),
        "gone after its 8 s: {:?}",
        h.drawn()
    );
}

#[test]
fn a_rescan_that_holds_the_window_still_says_what_it_found() {
    let mut h = Harness::new();
    let menu = Pos2::new(
        size::HEADER_PAD_X + size::MENU_BUTTON[0] / 2.0,
        size::HEADER / 2.0,
    );
    let _ = h.click(menu);
    h.idle(2);
    // The host describes new bundles on the window's thread, and answers five seconds after
    // the frame that asked.
    h.host.busy = true;
    let from = h.host.calls.len();
    let _ = h.click(h.text("Rescan plugins"));
    assert_eq!(h.host.calls[from..], [Call::Rescan]);
    h.host.busy = false;
    h.quiet(5.0);
    h.idle(1);
    let plugins = h.host.catalog().plugins.len();
    let said = format!("Rescanned the plugin folders: {plugins} plugins");
    assert!(h.find(&said).is_some(), "{:?}", h.drawn());
    // Its 2.4 s count from the answer.
    h.wait(2.0);
    assert!(h.find(&said).is_some(), "{:?}", h.drawn());
}

/// Opens the plugin browser from Game's "+ Add plugin", and lets it settle.
fn open_browser(h: &mut Harness) -> Strip {
    let game = Strip::Channel(h.channel(1));
    let _ = h.click(h.slot(game, 0).center());
    h.idle(2);
    assert!(matches!(h.shell.popover, Some(Popover::Browser(_))));
    game
}

#[test]
fn the_browser_s_arrows_reach_a_row_out_of_view_and_enter_adds_it() {
    let mut h = Harness::new();
    let game = open_browser(&mut h);
    // All plugins, A to Z: the last is far below the list's bottom, out of view.
    let last = "Youlean Loudness Meter 2";
    let list_bottom = h.text_rect("Adds to Game, slot 1").top();
    assert!(h.find(last).is_none_or(|at| at.y > list_bottom));
    let rows = h.host.catalog().plugins.len();
    for _ in 1..rows {
        let _ = h.key(Key::ArrowDown, Modifiers::NONE);
    }
    // Picked, it scrolled into view.
    h.wait(1.0);
    assert!(h.text_rect(last).bottom() <= list_bottom, "{:?}", h.drawn());
    let sent = h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        sent,
        vec![Command::AddInsert {
            strip: game,
            plugin: example::plugin(last),
            at: 0
        }]
    );
}

#[test]
fn tab_walks_the_browser_s_list_into_rows_out_of_view() {
    let mut h = Harness::new();
    let _ = open_browser(&mut h);
    let last = example::plugin("Youlean Loudness Meter 2").id;
    let row = egui::Id::new(("plugin", last.as_str()));
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    // From the search field through the nav and every row and star before it.
    let mut tabs = 0;
    while focused(&h) != Some(row) {
        assert!(tabs < 200, "Tab never reached the last row");
        let _ = h.key(Key::Tab, Modifiers::NONE);
        tabs += 1;
    }
    // It scrolled into view as it took the focus.
    h.wait(1.0);
    let list_bottom = h.text_rect("Adds to Game, slot 1").top();
    let at = h.text_rect("Youlean Loudness Meter 2");
    assert!(at.bottom() <= list_bottom, "{at:?} below {list_bottom}");
}

#[test]
fn an_open_browser_lists_what_a_rescan_found() {
    let mut h = Harness::new();
    let _ = open_browser(&mut h);
    assert!(h.find("Airwindows Consolidated").is_some());
    // The rescan found one plugin under a new name: as many plugins, in a new order.
    let mut catalog = h.host.inner.catalog().clone();
    if let Some(plugin) = catalog
        .plugins
        .iter_mut()
        .find(|p| p.name == "Youlean Loudness Meter 2")
    {
        plugin.name = "Aardvark Meter".to_string();
    }
    h.host.catalog = Some(catalog);
    h.host.news = vec![vixmix_host::HostEvent::PluginsRescanned];
    h.idle(1);
    let new = h.text_rect("Aardvark Meter");
    let next = h.text_rect("Airwindows Consolidated");
    assert!(new.top() < next.top(), "first, A to Z: {new:?} {next:?}");
}

/// The browser's list as it shows: right of the nav, between the search row and the footer.
fn browser_list(h: &Harness) -> egui::Rect {
    let area = browser_area(h);
    egui::Rect::from_min_max(
        Pos2::new(
            area.left() + pop::NAV_WIDTH,
            area.top() + pop::SEARCH_HEIGHT,
        ),
        Pos2::new(area.right(), area.bottom() - pop::BROWSER_FOOT),
    )
}

/// The browser's nav as it shows: left of the list, between the search row and the footer.
fn browser_nav(h: &Harness) -> egui::Rect {
    let area = browser_area(h);
    egui::Rect::from_min_max(
        Pos2::new(area.left(), area.top() + pop::SEARCH_HEIGHT),
        Pos2::new(
            area.left() + pop::NAV_WIDTH,
            area.bottom() - pop::BROWSER_FOOT,
        ),
    )
}

fn browser_area(h: &Harness) -> egui::Rect {
    let popover = h.shell.popover.as_ref().expect("a popover is open");
    assert!(matches!(popover, Popover::Browser(_)), "{popover:?}");
    h.ctx
        .memory(|m| m.area_rect(popover.area_id()))
        .expect("the browser was drawn")
}

/// Whether the last frame drew `text` inside `view` (the console's own texts are not the
/// browser's names: none of its inserts is one these tests look for).
fn shown(h: &Harness, view: egui::Rect, text: &str) -> bool {
    h.texts
        .iter()
        .any(|(t, rect, _)| t == text && view.contains_rect(*rect))
}

/// The wheel turned by `delta` points over `at` (less than zero scrolls down), and the
/// frames the wheel's smoothing takes to apply all of it.
fn wheel(h: &mut Harness, at: Pos2, delta: f32) {
    let _ = h.pass(vec![
        egui::Event::PointerMoved(at),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.idle(30);
}

/// The catalog's plugins as All plugins lists them, by name, case aside: name and id.
fn all_plugins(h: &Harness) -> Vec<(String, String)> {
    let mut all: Vec<_> = h
        .host
        .catalog()
        .plugins
        .iter()
        .map(|p| (p.name.clone(), p.id.clone()))
        .collect();
    all.sort_by_key(|(name, _)| name.to_lowercase());
    all
}

#[test]
fn typing_after_a_scroll_shows_the_first_hit_that_enter_adds() {
    let mut h = Harness::new();
    let game = open_browser(&mut h);
    let list = browser_list(&h);
    wheel(&mut h, list.center(), -5000.0);
    assert!(
        shown(&h, list, "Youlean Loudness Meter 2"),
        "scrolled to the end"
    );
    assert!(!shown(&h, list, "Airwindows Consolidated"));
    // "e": EchoBoy is the first hit, picked, and the list starts at its top to show it.
    let _ = h.type_text("e");
    h.idle(1);
    assert!(shown(&h, list, "EchoBoy"), "{:?}", h.drawn());
    let sent = h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        sent,
        vec![Command::AddInsert {
            strip: game,
            plugin: example::plugin("EchoBoy"),
            at: 0
        }]
    );
}

#[test]
fn typing_while_a_picked_row_scrolls_in_still_shows_the_first_hit() {
    let mut h = Harness::new();
    let _ = open_browser(&mut h);
    let list = browser_list(&h);
    // Down picks rows past the list's bottom, each scrolling in; a letter typed at once makes
    // a new list from its top, and no scroll still under way pulls it back down.
    for _ in 0..20 {
        let _ = h.key(Key::ArrowDown, Modifiers::NONE);
    }
    let _ = h.type_text("e");
    h.idle(30);
    assert!(shown(&h, list, "EchoBoy"), "{:?}", h.drawn());
}

#[test]
fn the_browser_opens_at_the_top_of_its_list_and_nav_on_any_strip() {
    let mut h = Harness::new();
    let _ = open_browser(&mut h);
    let (list, nav) = (browser_list(&h), browser_nav(&h));
    wheel(&mut h, list.center(), -5000.0);
    wheel(&mut h, nav.center(), -5000.0);
    assert!(!shown(&h, list, "Airwindows Consolidated"));
    assert!(!shown(&h, nav, "Favorites"));
    let _ = h.key(Key::Escape, Modifiers::NONE);
    assert_eq!(h.shell.popover, None);
    // Opened again from Mic's "+ Add plugin", the list and the nav start at their tops.
    let mic = Strip::Channel(h.channel(0));
    let len = h.setup().channels[0].chain.len();
    let _ = h.click(h.slot(mic, len).center());
    h.idle(1);
    let (list, nav) = (browser_list(&h), browser_nav(&h));
    assert!(shown(&h, list, "A"), "the first group's heading");
    assert!(
        shown(&h, list, "Airwindows Consolidated"),
        "{:?}",
        h.drawn()
    );
    assert!(shown(&h, nav, "Favorites"), "{:?}", h.drawn());
    // The row drawn picked is the row Enter adds.
    let sent = h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        sent,
        vec![Command::AddInsert {
            strip: mic,
            plugin: example::plugin("Airwindows Consolidated"),
            at: len
        }]
    );
}

#[test]
fn a_nav_filter_picked_after_a_scroll_shows_its_first_row() {
    let mut h = Harness::new();
    // Twenty more compressors: Dynamics is longer than the list shows.
    let mut catalog = h.host.inner.catalog().clone();
    let model = catalog
        .plugins
        .iter()
        .find(|p| p.name == "Pro-C 2")
        .cloned()
        .expect("Pro-C 2 is in the example catalog");
    for n in 1..=20 {
        let mut plugin = model.clone();
        plugin.id = format!("{}-{n}", model.id);
        plugin.name = format!("Zz Comp {n:02}");
        catalog.plugins.push(plugin);
    }
    let first = catalog
        .plugins
        .iter()
        .filter(|p| p.kind == model.kind)
        .map(|p| p.name.clone())
        .min_by_key(|name| name.to_lowercase())
        .expect("a compressor");
    h.host.catalog = Some(catalog);
    let _ = open_browser(&mut h);
    let (list, nav) = (browser_list(&h), browser_nav(&h));
    wheel(&mut h, list.center(), -5000.0);
    assert!(!shown(&h, list, &first));
    // The nav's Dynamics, not a row's type column.
    let dynamics = h
        .texts
        .iter()
        .find(|(t, rect, _)| t == "Dynamics" && nav.contains_rect(*rect))
        .map(|(_, rect, _)| rect.center())
        .expect("Dynamics in the nav");
    let _ = h.click(dynamics);
    h.idle(1);
    assert!(shown(&h, list, &first), "{first} in {:?}", h.drawn());
}

#[test]
fn a_focused_row_or_nav_item_keeps_the_focus_out_of_view_and_tab_goes_on_from_it() {
    let mut h = Harness::new();
    let _ = open_browser(&mut h);
    let (list, nav) = (browser_list(&h), browser_nav(&h));
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    // Tab from the search field reaches the nav's first item; the wheel takes it out of
    // view, and it keeps the focus.
    let _ = h.key(Key::Tab, Modifiers::NONE);
    let favorites = egui::Id::new(("nav", &crate::browser::Filter::Favorites));
    assert_eq!(focused(&h), Some(favorites));
    wheel(&mut h, nav.center(), -5000.0);
    assert!(!shown(&h, nav, "Favorites"));
    assert_eq!(focused(&h), Some(favorites), "the nav item keeps the focus");
    // Tab on to the list's first row; the wheel takes it out of view, and it keeps the focus.
    let all = all_plugins(&h);
    let (first, second) = (&all[0], &all[1]);
    let row = |id: &str| egui::Id::new(("plugin", id));
    let star = |id: &str| egui::Id::new(("star", id));
    let mut tabs = 0;
    while focused(&h) != Some(row(&first.1)) {
        assert!(
            tabs < 100,
            "Tab never reached the first row: {:?}",
            focused(&h)
        );
        let _ = h.key(Key::Tab, Modifiers::NONE);
        tabs += 1;
    }
    wheel(&mut h, list.center(), -5000.0);
    assert!(!shown(&h, list, &first.0));
    assert_eq!(focused(&h), Some(row(&first.1)), "the row keeps the focus");
    // Tab goes on to its star, which scrolls the row back in; then to the next row.
    let _ = h.key(Key::Tab, Modifiers::NONE);
    assert_eq!(focused(&h), Some(star(&first.1)));
    h.idle(1);
    assert!(shown(&h, list, &first.0), "{:?}", h.drawn());
    let _ = h.key(Key::Tab, Modifiers::NONE);
    assert_eq!(focused(&h), Some(row(&second.1)));
    // Out of view again, Shift+Tab goes back to the star before it.
    wheel(&mut h, list.center(), -5000.0);
    assert!(!shown(&h, list, &second.0));
    let _ = h.key(Key::Tab, Modifiers::SHIFT);
    h.idle(1);
    assert_eq!(focused(&h), Some(star(&first.1)));
}

#[test]
fn a_panel_s_list_opens_at_its_top_on_any_channel() {
    let mut h = Harness::new();
    let (music, browser) = (h.channel(2), h.channel(3));
    let _ = h.click(h.head(Strip::Channel(music)).center());
    h.idle(2);
    let top = h.text_rect("PLAYING NOW").top();
    let list = h.text_rect("Spotify").center();
    wheel(&mut h, list, -5000.0);
    assert!(
        h.text_rect("PLAYING NOW").top() < top,
        "the app list scrolled"
    );
    let _ = h.key(Key::Escape, Modifiers::NONE);
    // Another channel's panel, the same list: it starts at its top.
    let _ = h.click(h.head(Strip::Channel(browser)).center());
    h.idle(1);
    assert_eq!(h.text_rect("PLAYING NOW").top(), top);
}

/// Whether the last frame drew `text` whole in the open popover, inside what clips it: a row
/// its scrolled list shows.
fn in_view(h: &Harness, text: &str) -> bool {
    let popover = h.shell.popover.as_ref().expect("a popover is open");
    let area = h
        .ctx
        .memory(|m| m.area_rect(popover.area_id()))
        .expect("the popover was drawn");
    h.whole
        .iter()
        .any(|(t, rect)| t == text && area.contains_rect(*rect))
}

/// The example's devices and `extra` more inputs and outputs after them ("Line in 1",
/// "Line out 1" and on), as a machine with many answers: more than a panel's list shows.
/// Answers the inputs and the outputs, in the order the panels list them.
fn many_devices(h: &mut Harness, extra: usize) -> (Vec<DeviceRef>, Vec<DeviceRef>) {
    use vixmix_model::{DeviceDirection, DeviceInfo};
    let mut devices = h.host.inner.devices().to_vec();
    for n in 1..=extra {
        for (name, direction, input) in [
            (format!("Line in {n}"), DeviceDirection::Input, true),
            (format!("Line out {n}"), DeviceDirection::Output, false),
        ] {
            devices.push(DeviceInfo {
                device: example::device(&name, input),
                direction,
            });
        }
    }
    let of = |direction| {
        devices
            .iter()
            .filter(|d| d.direction == direction)
            .map(|d| d.device.clone())
            .collect::<Vec<_>>()
    };
    let lists = (of(DeviceDirection::Input), of(DeviceDirection::Output));
    h.host.devices = Some(devices);
    lists
}

#[test]
fn a_panel_shows_the_radio_it_opens_on_and_each_row_the_keys_reach() {
    let mut h = Harness::new();
    let (inputs, outputs) = many_devices(&mut h, 30);
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    // Synth plays the last input, far below the twelve rows its panel's list shows.
    let synth = h.channel(5);
    let pick = |device: &DeviceRef| Command::SetSource {
        ch: synth,
        source: Source::Hardware(Some(device.clone())),
    };
    let last = inputs.len() - 1;
    assert!(h.host.inner.apply(pick(&inputs[last])).unwrap().changed());
    let _ = h.click(h.head(Strip::Channel(synth)).center());
    h.idle(2);
    let row = |i: usize| popover::input_id(&inputs[i]);
    let name = |i: usize| inputs[i].description.as_str();
    assert_eq!(focused(&h), Some(row(last)));
    assert!(in_view(&h, name(last)), "the checked radio shows");
    // Up picks the row before it, shown; Down twice comes back and goes round to the first,
    // and the list scrolls up to it; Up goes round to the last, and the list scrolls down.
    assert_eq!(
        h.key(Key::ArrowUp, Modifiers::NONE),
        vec![pick(&inputs[last - 1])]
    );
    assert!(in_view(&h, name(last - 1)));
    assert_eq!(
        h.key(Key::ArrowDown, Modifiers::NONE),
        vec![pick(&inputs[last])]
    );
    assert_eq!(
        h.key(Key::ArrowDown, Modifiers::NONE),
        vec![pick(&inputs[0])]
    );
    assert_eq!(focused(&h), Some(row(0)));
    assert!(in_view(&h, name(0)), "round to the first, shown");
    assert!(!in_view(&h, name(last)));
    assert_eq!(
        h.key(Key::ArrowUp, Modifiers::NONE),
        vec![pick(&inputs[last])]
    );
    assert!(in_view(&h, name(last)), "round to the last, shown");
    // Tab walks the list from its first row, each row it reaches shown.
    assert_eq!(
        h.key(Key::ArrowDown, Modifiers::NONE),
        vec![pick(&inputs[0])]
    );
    for i in 1..=last {
        assert!(h.key(Key::Tab, Modifiers::NONE).is_empty());
        assert_eq!(focused(&h), Some(row(i)));
        assert!(in_view(&h, name(i)), "Tab to row {i}, shown");
    }
    let _ = h.key(Key::Escape, Modifiers::NONE);

    // Master's output panel, on its last output: shown, and Up picks the one before, shown.
    let set = |device: Option<&DeviceRef>| Command::SetMixDevice {
        mix: MixId::Master,
        device: device.cloned(),
    };
    let last = outputs.len() - 1;
    let name = |i: usize| outputs[i].description.as_str();
    assert!(
        h.host
            .inner
            .apply(set(Some(&outputs[last])))
            .unwrap()
            .changed()
    );
    let _ = h.click(h.head(Strip::Mix(MixId::Master)).center());
    h.idle(2);
    assert_eq!(focused(&h), Some(popover::output_id(Some(&outputs[last]))));
    assert!(in_view(&h, name(last)), "the checked output shows");
    assert_eq!(
        h.key(Key::ArrowUp, Modifiers::NONE),
        vec![set(Some(&outputs[last - 1]))]
    );
    assert!(in_view(&h, name(last - 1)));
    // Down goes back, then round to "No device", first, and the list scrolls up to it.
    let _ = h.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.key(Key::ArrowDown, Modifiers::NONE), vec![set(None)]);
    assert!(in_view(&h, "No device"), "round to No device, shown");
}

#[test]
fn shift_tab_through_sixteen_plugin_folders_shows_each() {
    let mut h = Harness::new();
    let folder = |i: usize| format!("D:/VST3/{i}");
    let prefs = vixmix_model::Prefs {
        plugin_dirs: (0..MAX_PLUGIN_DIRS)
            .map(|i| std::path::PathBuf::from(folder(i)))
            .collect(),
        ..h.setup().prefs.clone()
    };
    assert!(h.host.apply(Command::SetPrefs(prefs)).unwrap().changed());
    let menu = Pos2::new(
        size::HEADER_PAD_X + size::MENU_BUTTON[0] / 2.0,
        size::HEADER / 2.0,
    );
    let _ = h.click(menu);
    h.idle(2);
    let _ = h.click(h.text("Preferences"));
    h.idle(2);
    // From the field under the list, Shift+Tab goes back to the folders' bins, the last
    // first; each one it reaches shows, though the list holds more than it shows.
    let _ = h.click(h.text("Another folder to scan"));
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    let bin = |i: usize| egui::Id::new(("folder", folder(i).as_str()));
    let last = MAX_PLUGIN_DIRS - 1;
    assert!(
        !in_view(&h, &folder(last)),
        "the last folder starts out of view"
    );
    let mut tabs = 0;
    while focused(&h) != Some(bin(last)) {
        assert!(
            tabs < 4,
            "Shift+Tab never reached the last folder: {:?}",
            focused(&h)
        );
        let _ = h.key(Key::Tab, Modifiers::SHIFT);
        h.idle(1);
        tabs += 1;
    }
    assert!(
        in_view(&h, &folder(last)),
        "Shift+Tab to the last folder, shown"
    );
    for i in (0..last).rev() {
        let _ = h.key(Key::Tab, Modifiers::SHIFT);
        h.idle(1);
        assert_eq!(focused(&h), Some(bin(i)));
        assert!(in_view(&h, &folder(i)), "Shift+Tab to folder {i}, shown");
    }
}
