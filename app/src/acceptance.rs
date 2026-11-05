//! The window's gestures, each pressed, dragged or typed in a headless frame on
//! `FakeHost`'s example console, each asserting the command it sends (handoff 05's
//! acceptance), and what the setup looks like after.

use egui::{Key, Modifiers, Pos2, vec2};
use vixmix_model::{
    ChannelKind, Command, DeviceRef, MixId, SlotRef, Source, Strip, example, taper,
};

use vixmix_host::Host;

use crate::fader::{FINE_DRAG, FINE_KEY_STEP, KEY_STEP, Target};
use crate::popover::Popover;
use crate::shell::Intent;
use crate::test_support::{Call, Harness};
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
    let _ = h.click(star);
    assert!(h.intents.iter().any(|i| matches!(i, Intent::Favorite { id, on: false } if *id == example::plugin("Pro-Q 3").id)), "{:?}", h.intents);
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
    let sent = h.click(h.text("Undo"));
    assert_eq!(
        sent,
        vec![Command::AddInsert {
            strip: mic,
            plugin: removed.plugin.clone(),
            at: 1
        }]
    );
    assert_eq!(h.setup().channels[0].chain[1].plugin, removed.plugin);
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
    let Command::AddInsert { .. } = sent[0] else {
        panic!("{sent:?}")
    };
    let Command::SetInsertEnabled { instance, enabled } = sent[1] else {
        panic!("{sent:?}")
    };
    assert!(!enabled);
    assert_eq!(h.setup().channels[2].chain[0].instance, instance);
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
    assert!(h.find("Removed Music").is_some());
    let sent = h.click(h.text("Undo"));
    let back = h.setup().channels.last().unwrap().clone();
    assert_eq!(
        sent,
        vec![
            Command::AddChannel {
                kind: ChannelKind::Apps
            },
            Command::Rename {
                ch: back.id,
                name: "Music".to_string()
            },
            Command::SetSource {
                ch: back.id,
                source: music.source.clone()
            },
            Command::SetLink {
                ch: back.id,
                link: false
            },
            Command::SetSend {
                ch: back.id,
                mix: MixId::Monitor,
                position: music.monitor.position
            },
            Command::SetSend {
                ch: back.id,
                mix: MixId::Master,
                position: music.master.position
            },
            Command::SetMute {
                ch: back.id,
                mix: MixId::Master,
                mute: true
            },
            Command::AddInsert {
                strip: Strip::Channel(back.id),
                plugin: music.chain[0].plugin.clone(),
                at: 0
            },
            Command::SetInsertEnabled {
                instance: back.chain[0].instance,
                enabled: false
            },
        ]
    );
    // All of it back, but at the right end and with new ids (change request 2 restores
    // the place and the ids).
    assert_eq!(back.name, music.name);
    assert_eq!(back.source, music.source);
    assert_eq!((back.monitor, back.master), (music.monitor, music.master));
    assert_eq!(back.link, music.link);
    assert_eq!(back.chain[0].plugin, music.chain[0].plugin);
    assert!(!back.chain[0].enabled);
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
