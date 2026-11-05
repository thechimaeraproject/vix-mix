//! The plugin browser (`.br`, 620 x 440), beside the "+ Add plugin" that opened it.
//! Search-first: the field has the focus, typing searches every plugin by name, maker or type
//! (ranked as the prototype ranks: the name starts with it, then contains it, then the maker,
//! then the type), Up and Down pick a row and Enter adds it. With the field empty, the nav
//! picks the list: Favorites, Recent and All plugins with their counts, then each Type and
//! each Maker; All is grouped A to Z. A row's star toggles the favourite; a press on the row
//! adds it. The footer says where it goes: "Adds to <strip>, slot N".

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, EventFilter, Id, Key, Rect, ScrollArea, Sense, Stroke, Ui, Vec2, pos2, vec2};
use vixmix_host::Host;
use vixmix_model::{Command, PluginRef, Strip};

use crate::icons;
use crate::popover::{FIELD_LIMIT, Outcome, PopCx, search_id};
use crate::shell::Intent;
use crate::strip::{self, Spacing};
use crate::theme::{colour, font, pop, size};
use crate::widgets;

/// What the nav picks while the search field is empty.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Favorites,
    Recent,
    All,
    /// A type, by its label (`Dynamics`).
    Kind(String),
    /// A maker.
    Vendor(String),
}

/// The open browser.
#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
    /// The strip the plugin goes to, at the end of its chain.
    pub strip: Strip,
    pub query: String,
    pub filter: Filter,
    /// The picked row, which Enter adds.
    pub active: usize,
    /// Its first frame: the focus goes to the search field.
    pub fresh: bool,
    /// The picked row moved by a key: scroll it into view.
    pub follow: bool,
}

impl Browser {
    /// A browser adding to `strip`, everything listed.
    pub fn new(strip: Strip) -> Browser {
        Browser {
            strip,
            query: String::new(),
            filter: Filter::All,
            active: 0,
            fresh: true,
            follow: false,
        }
    }
}

/// One plugin as the list shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub vendor: &'a str,
    pub kind: &'static str,
    pub bundle: &'a str,
}

impl Row<'_> {
    /// How a setup stores this plugin.
    pub fn plugin(&self) -> PluginRef {
        PluginRef {
            id: self.id.to_string(),
            bundle: self.bundle.to_string(),
            name: self.name.to_string(),
        }
    }
}

/// Every plugin the catalog has, as rows.
fn all(host: &dyn Host) -> Vec<Row<'_>> {
    host.catalog()
        .plugins
        .iter()
        .map(|p| Row {
            id: &p.id,
            name: &p.name,
            vendor: &p.vendor,
            kind: p.kind.label(),
            bundle: &p.bundle,
        })
        .collect()
}

fn by_name(a: &Row<'_>, b: &Row<'_>) -> std::cmp::Ordering {
    a.name.to_lowercase().cmp(&b.name.to_lowercase())
}

/// The list for `query` and `filter`: a query searches everything, whatever the filter,
/// ranked (name starts with it, name contains it, maker, type; each by name); otherwise the
/// filter's plugins by name, Recent in its own order.
pub fn rows<'a>(
    host: &'a dyn Host,
    query: &str,
    filter: &Filter,
    favorite: &dyn Fn(&str) -> bool,
) -> Vec<Row<'a>> {
    let q = query.trim().to_lowercase();
    let mut list = all(host);
    if !q.is_empty() {
        let score = |r: &Row<'_>| {
            let name = r.name.to_lowercase();
            if name.starts_with(&q) {
                Some(0)
            } else if name.contains(&q) {
                Some(1)
            } else if r.vendor.to_lowercase().contains(&q) {
                Some(2)
            } else if r.kind.to_lowercase().contains(&q) {
                Some(3)
            } else {
                None
            }
        };
        let mut hits: Vec<(u8, Row<'a>)> = list
            .into_iter()
            .filter_map(|r| score(&r).map(|s| (s, r)))
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| by_name(&a.1, &b.1)));
        return hits.into_iter().map(|(_, r)| r).collect();
    }
    match filter {
        Filter::Recent => {
            return host
                .catalog()
                .recent
                .iter()
                .filter_map(|id| list.iter().find(|r| r.id == id).copied())
                .collect();
        }
        Filter::Favorites => list.retain(|r| favorite(r.id)),
        Filter::All => {}
        Filter::Kind(kind) => list.retain(|r| r.kind == kind),
        Filter::Vendor(vendor) => list.retain(|r| r.vendor == vendor),
    }
    list.sort_by(by_name);
    list
}

/// The nav's entries: Favorites, Recent, All plugins; then each type the catalog has, in
/// the prototype's order; then each maker by name. Each with its count.
pub fn nav(host: &dyn Host, favorite: &dyn Fn(&str) -> bool) -> Vec<(Filter, String, usize)> {
    let catalog = host.catalog();
    let plugins = &catalog.plugins;
    let mut out = vec![
        (
            Filter::Favorites,
            "Favorites".to_string(),
            plugins.iter().filter(|p| favorite(&p.id)).count(),
        ),
        (
            Filter::Recent,
            "Recent".to_string(),
            catalog
                .recent
                .iter()
                .filter(|id| plugins.iter().any(|p| &p.id == *id))
                .count(),
        ),
        (Filter::All, "All plugins".to_string(), plugins.len()),
    ];
    let mut kinds: Vec<_> = plugins.iter().map(|p| p.kind).collect();
    kinds.sort();
    kinds.dedup();
    for kind in kinds {
        let label = kind.label();
        let count = plugins.iter().filter(|p| p.kind == kind).count();
        out.push((Filter::Kind(label.to_string()), label.to_string(), count));
    }
    for (vendor, count) in catalog.vendors() {
        out.push((
            Filter::Vendor(vendor.to_string()),
            vendor.to_string(),
            count,
        ));
    }
    out
}

/// The A to Z group a name falls in: its first letter, upper case, or `#`.
pub fn group(name: &str) -> char {
    match name.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase(),
        _ => '#',
    }
}

/// The browser's frame: the search row, the nav and the list, the footer.
pub fn show(ui: &mut Ui, browser: &mut Browser, cx: &mut PopCx<'_>) -> Outcome {
    let host = cx.host;
    let favorite = cx.favorites;
    let (rect, _) = ui.allocate_exact_size(pop::BROWSER.into(), Sense::hover());
    let search = Rect::from_min_size(rect.min, vec2(rect.width(), pop::SEARCH_HEIGHT));
    let foot = Rect::from_min_max(
        pos2(rect.left(), rect.bottom() - pop::BROWSER_FOOT),
        rect.max,
    );
    let body = Rect::from_min_max(
        pos2(rect.left(), search.bottom()),
        pos2(rect.right(), foot.top()),
    );
    let nav_rect = Rect::from_min_size(body.min, vec2(pop::NAV_WIDTH, body.height()));
    let list_rect = Rect::from_min_max(pos2(nav_rect.right(), body.top()), body.max);

    // The search row.
    let fresh = std::mem::take(&mut browser.fresh);
    let field_id = search_id("plugins");
    let painter = ui.painter().clone();
    widgets::rule(&painter, search.x_range(), search.bottom() - size::HAIRLINE);
    let y = search.center().y - size::HAIRLINE_INSET;
    let icon = Rect::from_center_size(
        pos2(
            search.left() + pop::SEARCH_PAD_X + pop::SEARCH_ICON / 2.0,
            y,
        ),
        vec2(pop::SEARCH_ICON, pop::SEARCH_ICON),
    );
    icons::paint(&painter, &icons::SEARCH, icon, colour::FAINT);
    let hint = strip::galley(
        &painter,
        "\u{2191} \u{2193} to pick \u{b7} Enter to add",
        font::mono(font::KBD),
        colour::FAINT,
        Spacing::None,
    );
    let hint_width = hint.size().x;
    strip::place(
        &painter,
        hint,
        Align2::RIGHT_CENTER,
        pos2(search.right() - pop::SEARCH_PAD_X, y),
    );
    let field = Rect::from_min_max(
        pos2(icon.right() + pop::SEARCH_GAP, y - pop::FIELD_HEIGHT / 2.0),
        pos2(
            search.right() - pop::SEARCH_PAD_X - hint_width - pop::SEARCH_GAP,
            y + pop::FIELD_HEIGHT / 2.0,
        ),
    );
    let before = browser.query.clone();
    let placeholder = format!(
        "Search {} plugins by name, maker or type",
        host.catalog().plugins.len()
    );
    browser.query.truncate(FIELD_LIMIT);
    let response = widgets::text_field(
        ui,
        field,
        field_id,
        &mut browser.query,
        font::sans(font::SEARCH),
        &placeholder,
    );
    if fresh {
        ui.memory_mut(|m| m.request_focus(field_id));
    }
    if browser.query != before {
        browser.filter = Filter::All;
        browser.active = 0;
    }

    let list = rows(host, &browser.query, &browser.filter, favorite);
    let mut outcome = Outcome::Stay;
    let mut add = None;
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                field_id,
                EventFilter {
                    vertical_arrows: true,
                    ..EventFilter::default()
                },
            );
        });
        let (down, up) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp)));
        if down || up {
            let last = list.len().saturating_sub(1);
            browser.active = if down {
                (browser.active + 1).min(last)
            } else {
                browser.active.saturating_sub(1)
            };
            browser.follow = true;
        }
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        add = list.get(browser.active).copied();
    }
    browser.active = browser.active.min(list.len().saturating_sub(1));

    // The nav.
    painter.vline(
        nav_rect.right() - size::HAIRLINE_INSET,
        nav_rect.y_range(),
        Stroke::new(size::HAIRLINE, colour::LINE),
    );
    let entries = nav(host, favorite);
    let mut picked = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(nav_rect.shrink(pop::LIST_PAD)));
    ScrollArea::vertical()
        .id_salt("browser nav")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let mut section = 0;
            for (filter, label, count) in &entries {
                let heading = match filter {
                    Filter::Kind(_) if section < 1 => Some("Type"),
                    Filter::Vendor(_) if section < 2 => Some("Maker"),
                    _ => None,
                };
                if let Some(heading) = heading {
                    section = if heading == "Type" { 1 } else { 2 };
                    nav_heading(ui, heading);
                }
                let on = browser.query.trim().is_empty() && *filter == browser.filter;
                if nav_item(ui, filter, label, *count, on, cx.keyboard) {
                    picked = Some(filter.clone());
                }
            }
        });
    if let Some(filter) = picked {
        browser.filter = filter;
        browser.query.clear();
        browser.active = 0;
    }

    // The list.
    let mut star = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list_rect.shrink(pop::LIST_PAD)));
    let follow = std::mem::take(&mut browser.follow);
    ScrollArea::vertical()
        .id_salt("browser list")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if list.is_empty() {
                let text = format!(
                    "No installed plugin matches \u{201c}{}\u{201d}.",
                    browser.query.trim()
                );
                empty(ui, &text);
            }
            let grouped = browser.query.trim().is_empty() && browser.filter == Filter::All;
            let mut letter = None;
            for (index, row) in list.iter().enumerate() {
                if grouped && letter != Some(group(row.name)) {
                    letter = Some(group(row.name));
                    group_heading(ui, group(row.name));
                }
                let active = index == browser.active;
                let (rect, pressed, starred) = plugin_row(
                    ui,
                    row,
                    &browser.query,
                    active,
                    favorite(row.id),
                    cx.keyboard,
                );
                if active && follow {
                    ui.scroll_to_rect(rect, None);
                }
                if starred {
                    star = Some(*row);
                } else if pressed {
                    add = Some(*row);
                }
            }
        });
    if let Some(row) = star {
        cx.out.push(Intent::Favorite {
            id: row.id.to_string(),
            on: !favorite(row.id),
        });
    }

    // The footer.
    widgets::rule(&painter, foot.x_range(), foot.top());
    let setup = host.setup();
    let (name, len) = match browser.strip {
        Strip::Channel(ch) => match setup.channel(ch) {
            Some(channel) => (channel.name.as_str(), channel.chain.len()),
            None => return Outcome::Drop,
        },
        Strip::Mix(mix) => (mix.name(), setup.mix(mix).chain.len()),
    };
    footer(&painter, foot, name, len + 1);

    if let Some(row) = add {
        cx.out.push(Intent::Apply(Command::AddInsert {
            strip: browser.strip,
            plugin: row.plugin(),
            at: len,
        }));
        outcome = Outcome::Close;
    }
    outcome
}

/// "Adds to <strip>, slot N".
fn footer(painter: &egui::Painter, rect: Rect, name: &str, slot: usize) {
    let mut job = LayoutJob::default();
    let plain = TextFormat {
        font_id: font::sans(font::SMALL),
        color: colour::DIM,
        ..TextFormat::default()
    };
    job.append("Adds to ", 0.0, plain.clone());
    job.append(
        name,
        0.0,
        TextFormat {
            font_id: font::sans_semibold(font::SMALL),
            color: colour::TEXT,
            ..TextFormat::default()
        },
    );
    job.append(&format!(", slot {slot}"), 0.0, plain);
    job.wrap = TextWrapping::no_max_width();
    let galley = painter.layout_job(job);
    strip::place(
        painter,
        galley,
        Align2::LEFT_CENTER,
        pos2(
            rect.left() + pop::FOOT_PAD_LEFT,
            rect.center().y + size::HAIRLINE_INSET,
        ),
    );
}

/// A nav heading (`.nav-h`).
fn nav_heading(ui: &mut Ui, text: &str) {
    let painter = ui.painter().clone();
    let line = widgets::heading_height(&painter);
    let [top, bottom, across] = pop::NAV_HEADING;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), top + line + bottom),
        Sense::hover(),
    );
    widgets::heading(
        &painter,
        rect.left() + across,
        rect.top() + top + line / 2.0,
        text,
    );
}

/// An A to Z group heading (`.br-group`).
fn group_heading(ui: &mut Ui, letter: char) {
    let painter = ui.painter().clone();
    let line = widgets::heading_height(&painter);
    let [top, bottom, across] = pop::GROUP;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), top + line + bottom),
        Sense::hover(),
    );
    widgets::heading(
        &painter,
        rect.left() + across,
        rect.top() + top + line / 2.0,
        &letter.to_string(),
    );
}

/// A nav item (`.nav-i`): the label and its count; returns whether it was pressed.
fn nav_item(
    ui: &mut Ui,
    filter: &Filter,
    label: &str,
    count: usize,
    on: bool,
    keyboard: bool,
) -> bool {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), pop::NAV_ROW), Sense::hover());
    let response = widgets::press(ui, rect, Id::new(("nav", filter)));
    let painter = ui.painter();
    let (fill, ink) = if on {
        (Some(colour::CTL_HI), colour::TEXT)
    } else if response.hovered() {
        (Some(colour::CTL), colour::TEXT)
    } else {
        (None, colour::DIM)
    };
    if let Some(fill) = fill {
        painter.rect_filled(rect, size::CORNER, fill);
    }
    let y = rect.center().y;
    let count = strip::galley(
        painter,
        &count.to_string(),
        font::mono(font::NOTE),
        colour::FAINT,
        Spacing::None,
    );
    let count_width = count.size().x;
    strip::place(
        painter,
        count,
        Align2::RIGHT_CENTER,
        pos2(rect.right() - pop::NAV_PAD_X, y),
    );
    let x = rect.left() + pop::NAV_PAD_X;
    let text = strip::truncated(
        painter,
        label,
        font::sans(font::BODY),
        ink,
        rect.right() - pop::NAV_PAD_X - count_width - pop::ROW_GAP - x,
    );
    strip::place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
    if keyboard && response.has_focus() {
        widgets::ring(painter, rect, 0.0);
    }
    response.clicked()
}

/// A plugin row (`.br-row`): the star, the name with the query's match in the accent, the
/// maker, the type. Returns its rect, whether the row was pressed, whether the star was.
fn plugin_row(
    ui: &mut Ui,
    row: &Row<'_>,
    query: &str,
    active: bool,
    starred: bool,
    keyboard: bool,
) -> (Rect, bool, bool) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), pop::PLUGIN_ROW), Sense::hover());
    let response = widgets::press(ui, rect, Id::new(("plugin", row.id)));
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, size::CORNER, colour::ACTIVE_ROW);
    } else if response.hovered() {
        painter.rect_filled(rect, size::CORNER, colour::CTL);
    }
    let y = rect.center().y;
    let left = rect.left() + pop::PLUGIN_PAD_X;
    let right = rect.right() - pop::PLUGIN_PAD_X;
    let star_rect = Rect::from_min_size(
        pos2(left, y - pop::STAR_COLUMN / 2.0),
        vec2(pop::STAR_COLUMN, pop::STAR_COLUMN),
    );
    let star = widgets::press(ui, star_rect, Id::new(("star", row.id))).on_hover_text(if starred {
        "Remove from favorites"
    } else {
        "Add to favorites"
    });
    let icon = Rect::from_center_size(star_rect.center(), vec2(pop::STAR, pop::STAR));
    if starred {
        icons::paint_filled(painter, &icons::STAR, icon, colour::ACCENT);
    } else {
        icons::paint(painter, &icons::STAR, icon, colour::FAINT);
    }
    let kind_left = right - pop::KIND_COLUMN;
    let vendor_left = kind_left - pop::PLUGIN_GAP - pop::VENDOR_COLUMN;
    let name_left = star_rect.right() + pop::PLUGIN_GAP;
    let name = highlighted(
        painter,
        row.name,
        query,
        vendor_left - pop::PLUGIN_GAP - name_left,
    );
    strip::place(painter, name, Align2::LEFT_CENTER, pos2(name_left, y));
    for (text, x, width) in [
        (row.vendor, vendor_left, pop::VENDOR_COLUMN),
        (row.kind, kind_left, pop::KIND_COLUMN),
    ] {
        let galley = strip::truncated(
            painter,
            text,
            font::sans(font::COLUMN),
            colour::FAINT,
            width,
        );
        strip::place(painter, galley, Align2::LEFT_CENTER, pos2(x, y));
    }
    if keyboard && response.has_focus() {
        widgets::ring(painter, rect, 0.0);
    }
    (rect, response.clicked(), star.clicked())
}

/// A plugin's name with the first match of `query` in the accent, cut to `width`.
fn highlighted(
    painter: &egui::Painter,
    name: &str,
    query: &str,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let format = |color| TextFormat {
        font_id: font::sans(font::BODY),
        color,
        ..TextFormat::default()
    };
    let mut job = LayoutJob::default();
    let q = query.trim().to_lowercase();
    let at = (!q.is_empty())
        .then(|| name.to_lowercase().find(&q))
        .flatten()
        // Lower-casing can move byte offsets; only a match on a char boundary is marked.
        .filter(|at| name.is_char_boundary(*at) && name.is_char_boundary(*at + q.len()));
    match at {
        Some(at) => {
            job.append(&name[..at], 0.0, format(colour::TEXT));
            job.append(&name[at..at + q.len()], 0.0, format(colour::ACCENT));
            job.append(&name[at + q.len()..], 0.0, format(colour::TEXT));
        }
        None => job.append(name, 0.0, format(colour::TEXT)),
    }
    job.wrap = TextWrapping::truncate_at_width(width.max(0.0));
    painter.layout_job(job)
}

/// The empty list's message.
fn empty(ui: &mut Ui, text: &str) {
    let painter = ui.painter().clone();
    let [pad_x, pad_y] = pop::EMPTY_PAD;
    let galley = strip::galley(
        &painter,
        text,
        font::sans(font::BODY),
        colour::DIM,
        Spacing::None,
    );
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + 2.0 * pad_y),
        Sense::hover(),
    );
    strip::place(
        &painter,
        galley,
        Align2::LEFT_CENTER,
        pos2(rect.left() + pad_x, rect.center().y),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_host::FakeHost;

    fn names(rows: &[Row<'_>]) -> Vec<String> {
        rows.iter().map(|r| r.name.to_string()).collect()
    }

    #[test]
    fn a_search_ranks_name_starts_then_contains_then_maker_then_type() {
        let host = FakeHost::new();
        let none = |_: &str| false;
        // "pro": names that start with it (FabFilter's), each by name; no name only contains it.
        // The filter does not narrow a search.
        let pro = rows(&host, "pro", &Filter::Favorites, &none);
        assert_eq!(
            names(&pro)[..8],
            [
                "Pro-C 2",
                "Pro-DS",
                "Pro-G",
                "Pro-L 2",
                "Pro-MB",
                "Pro-Q 3",
                "Pro-R 2",
                "Auto-Tune Pro"
            ]
        );
        // Then MeldaProduction's, by their maker.
        assert_eq!(names(&pro)[8..], ["MAutoPitch", "MEqualizer"]);
        // "verb": names that contain it, then nothing else (no maker or type has it bare).
        let verb = rows(&host, "verb", &Filter::All, &none);
        assert!(names(&verb).contains(&"ValhallaVintageVerb".to_string()));
        // "fabfilter": the maker, by name.
        let fab = rows(&host, "fabfilter", &Filter::All, &none);
        assert_eq!(fab.len(), 9);
        assert_eq!(fab[0].name, "Pro-C 2");
        // "reverb": names with it first, then the rest of the Reverb type.
        let reverb = rows(&host, "reverb", &Filter::All, &none);
        assert_eq!(reverb[0].name, "Dragonfly Hall Reverb");
        assert!(reverb.iter().any(|r| r.name == "Pro-R 2"), "by its type");
        assert!(rows(&host, "zzz", &Filter::All, &none).is_empty());
    }

    #[test]
    fn the_nav_counts_favorites_recent_types_and_makers() {
        let host = FakeHost::new();
        let catalog = host.catalog();
        let favorite = |id: &str| catalog.favorites.contains(id);
        let entries = nav(&host, &favorite);
        let count = |label: &str| {
            entries
                .iter()
                .find(|(_, l, _)| l == label)
                .map(|(_, _, c)| *c)
        };
        assert_eq!(count("Favorites"), Some(4));
        assert_eq!(count("Recent"), Some(5));
        assert_eq!(count("All plugins"), Some(42));
        assert_eq!(count("Dynamics"), Some(12));
        assert_eq!(count("FabFilter"), Some(9));
        // The types come in the prototype's order, before the makers.
        let labels: Vec<&str> = entries.iter().map(|(_, l, _)| l.as_str()).collect();
        assert_eq!(labels[3..6], ["EQ", "Dynamics", "Reverb"]);
        assert_eq!(labels[12], "Utility");
        // Recent keeps its own order; Favorites by name.
        let recent = rows(&host, "", &Filter::Recent, &favorite);
        assert_eq!(names(&recent)[..2], ["Pro-L 2", "OTT"]);
        let favorites = rows(&host, "", &Filter::Favorites, &favorite);
        assert_eq!(
            names(&favorites),
            ["Noise Suppression for Voice", "OTT", "Pro-L 2", "Pro-Q 3"]
        );
    }

    #[test]
    fn all_plugins_group_by_their_first_letter() {
        assert_eq!(group("kHs Chorus"), 'K');
        assert_eq!(group("Pro-Q 3"), 'P');
        assert_eq!(group("2C Audio"), '#');
    }
}
