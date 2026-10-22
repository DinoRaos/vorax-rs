//! Where each part of the Surgical Preparation screen sits (src/solver/scan/layout.ts): measured on 3440×1440
//! (21:9) frames and placed on others by scaling with the height from the anchor the UI uses. The tanks are in
//! the 3D scene, measured on 21:9 and 16:9 and blended between.

use crate::pixels::Rect;

const REF_H: f64 = 1440.0;

struct TankScene {
    x: [f64; 6],
    name: [f64; 2],
    total: [f64; 2],
    aq: [f64; 2],
    icon: f64,
    icon_size: f64,
    /// Not in upstream, for the overlay's marks: each tank's glass, its centre (nearer the middle of the screen
    /// than its label), top and bottom, and half its width. Measured on 16:9 frames; no 21:9 frame shows the
    /// tanks, so there it's 16:9's scaled by the tanks' wider spacing (×1.25) up from the labels.
    glass_x: [f64; 6],
    glass_y: [f64; 2],
    glass_half_w: f64,
}
const ULTRAWIDE: TankScene = TankScene {
    x: [-858.0, -421.0, 16.0, 454.0, 913.0, 1386.0],
    name: [556.0, 577.0],
    total: [578.0, 600.0],
    aq: [599.0, 619.0],
    icon: 533.0,
    icon_size: 26.0,
    glass_x: [-817.0, -393.0, 31.0, 455.0, 900.0, 1358.0],
    glass_y: [148.0, 486.0],
    glass_half_w: 144.0,
};
const WIDE_16_9: TankScene = TankScene {
    x: [-756.0, -400.0, -48.0, 304.0, 667.0, 1037.0],
    name: [511.0, 531.0],
    total: [532.0, 552.0],
    aq: [554.0, 574.0],
    icon: 496.0,
    icon_size: 22.0,
    glass_x: [-721.0, -376.0, -35.0, 307.0, 659.0, 1017.0],
    glass_y: [185.0, 455.0],
    glass_half_w: 115.0,
};
const LABEL_HALF_W: f64 = 200.0;

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn scene(aspect: f64) -> TankScene {
    let t = ((aspect - 16.0 / 9.0) / (3440.0 / 1440.0 - 16.0 / 9.0)).max(0.0).min(1.0);
    let pair = |a: [f64; 2], b: [f64; 2]| [mix(a[0], b[0], t), mix(a[1], b[1], t)];
    TankScene {
        x: std::array::from_fn(|i| mix(WIDE_16_9.x[i], ULTRAWIDE.x[i], t)),
        name: pair(WIDE_16_9.name, ULTRAWIDE.name),
        total: pair(WIDE_16_9.total, ULTRAWIDE.total),
        aq: pair(WIDE_16_9.aq, ULTRAWIDE.aq),
        icon: mix(WIDE_16_9.icon, ULTRAWIDE.icon, t),
        icon_size: mix(WIDE_16_9.icon_size, ULTRAWIDE.icon_size, t),
        glass_x: std::array::from_fn(|i| mix(WIDE_16_9.glass_x[i], ULTRAWIDE.glass_x[i], t)),
        glass_y: pair(WIDE_16_9.glass_y, ULTRAWIDE.glass_y),
        glass_half_w: mix(WIDE_16_9.glass_half_w, ULTRAWIDE.glass_half_w, t),
    }
}

const CARD_X0: f64 = -90.0;
const CARD_PITCH: f64 = 226.5;
const CARD_ART: (f64, f64, f64) = (975.0, 100.0, 120.0);
const CARD_FACE: (f64, f64, f64) = (960.0, 230.0, 340.0);
const CARD_CY: f64 = 1130.0;
const CARD_TILT: [f64; 3] = [-0.0576, 0.0, 0.0506];
const CARD_SHIFT: [(f64, f64); 3] = [(6.0, 6.0), (0.0, 0.0), (-6.0, 6.0)];
const HAND5_X: [f64; 5] = [-385.0, -163.0, 70.0, 303.0, 515.0];
const HAND5_Y: [f64; 5] = [1141.0, 1121.0, 1111.0, 1121.0, 1141.0];
const HAND5_TILT: [f64; 5] = [-0.11, -0.06, 0.0, 0.05, 0.11];
/// Art and title in a card's own frame, in reference pixels from its centre.
pub const CARD_LOCAL_ART: Rect = Rect::new(-50.0, -155.0, 100.0, 120.0);
pub const CARD_LOCAL_TEXT: Rect = Rect::new(-100.0, -45.0, 185.0, 175.0);
const HEADER: Rect = Rect::new(-260.0, 908.0, 560.0, 30.0);
const REFRESH: Rect = Rect::new(300.0, 1342.0, 160.0, 40.0);
/// From the left edge.
const ROUND: Rect = Rect::new(105.0, 500.0, 100.0, 70.0);
/// From the right edge: icon centres, the newest first.
const INSTRUMENT_RIGHT: f64 = 162.0;
const INSTRUMENT_PITCH: f64 = 135.0;
const INSTRUMENT_Y: f64 = 66.0;
const INSTRUMENT_SIZE: f64 = 74.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardPos {
    pub cx: f64,
    pub cy: f64,
    pub angle: f64,
}

/// A card's face around its centre.
pub fn card_face(c: &CardPos, k: f64) -> Rect {
    Rect::new(c.cx - (CARD_FACE.1 / 2.0) * k, c.cy - (CARD_FACE.2 / 2.0) * k, CARD_FACE.1 * k, CARD_FACE.2 * k)
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    /// pixels per reference pixel of the interface
    pub k: f64,
    pub tank_cx: [f64; 6],
    pub icon: [Rect; 6],
    pub name_band: [Rect; 6],
    pub total_band: [Rect; 6],
    pub aq_band: [Rect; 6],
    /// each tank's glass (not in upstream: the overlay outlines the tanks to target). In the 3D scene, so the
    /// interface scale doesn't move it.
    pub tank_glass: [Rect; 6],
    pub card_art: [Rect; 3],
    pub card_face: [Rect; 3],
    pub cards: [CardPos; 3],
    pub hand5: [CardPos; 5],
    pub header: Rect,
    pub refresh: Rect,
    pub round: Rect,
    /// rightmost slot first
    pub instrument_icon: [Rect; 4],
}

impl Layout {
    /// The same screen with the hand (its header and cards) `dy` reference pixels lower.
    pub fn shifted_hand(&self, dy: f64) -> Layout {
        let d = dy * self.k;
        let mut l = self.clone();
        l.header.y += d;
        for r in l.card_art.iter_mut().chain(l.card_face.iter_mut()) {
            r.y += d;
        }
        for c in l.cards.iter_mut().chain(l.hand5.iter_mut()) {
            c.cy += d;
        }
        l
    }

    /// The card positions for a hand of this size.
    pub fn hand_at(&self, size: usize) -> &[CardPos] {
        if size == 5 {
            &self.hand5
        } else {
            &self.cards
        }
    }
}

/// The game's interface scale: Windows' display scale (or the game's own) enlarges every panel, card and label
/// from where it's anchored, while the 3D tanks stay put (upstream's issue #12: 125% at 2560x1440).
pub const UI_SCALES: [f64; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];
/// The tanks' labels grow about their total's row.
const LABEL_MID: f64 = 542.0;

/// The layout at the interface's own scale (100%).
pub fn layout(width: usize, height: usize) -> Layout {
    layout_at(width, height, 1.0)
}

/// The layout with the interface at scale `ui` (UI_SCALES).
pub fn layout_at(width: usize, height: usize, ui: f64) -> Layout {
    let (wf, hf) = (width as f64, height as f64);
    let s = hf / REF_H;
    let k = s * ui;
    let cx = wf / 2.0;
    // a y from the bottom edge, from the middle, and in a tank's label
    let bottom = |y: f64| (REF_H - (REF_H - y) * ui) * s;
    let middle = |y: f64| (REF_H / 2.0 + (y - REF_H / 2.0) * ui) * s;
    let label = |y: f64| (LABEL_MID + (y - LABEL_MID) * ui) * s;
    // the labels' text grows, but not the space between tanks: a wider band would take in the next label
    let band = |x: f64, y: [f64; 2]| Rect::new(x - LABEL_HALF_W * s, label(y[0]), 2.0 * LABEL_HALF_W * s, (y[1] - y[0]) * k);
    let sc = scene(wf / hf);
    let tank_cx: [f64; 6] = std::array::from_fn(|i| cx + sc.x[i] * s);
    let card_cx: [f64; 3] = std::array::from_fn(|j| cx + (CARD_X0 + CARD_PITCH * j as f64) * k);
    let rect = |r: Rect, x0: f64, y: f64| Rect::new(x0 + r.x * k, y, r.w * k, r.h * k);
    Layout {
        width,
        height,
        k,
        tank_cx,
        icon: std::array::from_fn(|i| Rect::new(tank_cx[i] - (sc.icon_size / 2.0) * k, label(sc.icon) - (sc.icon_size / 2.0) * k, sc.icon_size * k, sc.icon_size * k)),
        name_band: std::array::from_fn(|i| band(tank_cx[i], sc.name)),
        total_band: std::array::from_fn(|i| band(tank_cx[i], sc.total)),
        aq_band: std::array::from_fn(|i| band(tank_cx[i], sc.aq)),
        tank_glass: std::array::from_fn(|i| Rect::new(cx + (sc.glass_x[i] - sc.glass_half_w) * s, sc.glass_y[0] * s, 2.0 * sc.glass_half_w * s, (sc.glass_y[1] - sc.glass_y[0]) * s)),
        card_art: std::array::from_fn(|j| Rect::new(card_cx[j] - (CARD_ART.1 / 2.0) * k, bottom(CARD_ART.0), CARD_ART.1 * k, CARD_ART.2 * k)),
        card_face: std::array::from_fn(|j| Rect::new(card_cx[j] - (CARD_FACE.1 / 2.0) * k, bottom(CARD_FACE.0), CARD_FACE.1 * k, CARD_FACE.2 * k)),
        cards: std::array::from_fn(|j| CardPos { cx: card_cx[j] + CARD_SHIFT[j].0 * k, cy: bottom(CARD_CY + CARD_SHIFT[j].1), angle: CARD_TILT[j] }),
        hand5: std::array::from_fn(|j| CardPos { cx: cx + HAND5_X[j] * k, cy: bottom(HAND5_Y[j]), angle: HAND5_TILT[j] }),
        header: rect(HEADER, cx, bottom(HEADER.y)),
        refresh: rect(REFRESH, cx, bottom(REFRESH.y)),
        round: rect(ROUND, 0.0, middle(ROUND.y)),
        instrument_icon: std::array::from_fn(|j| {
            let x = wf - (INSTRUMENT_RIGHT + INSTRUMENT_PITCH * j as f64) * k;
            Rect::new(x - (INSTRUMENT_SIZE / 2.0) * k, (INSTRUMENT_Y - INSTRUMENT_SIZE / 2.0) * k, INSTRUMENT_SIZE * k, INSTRUMENT_SIZE * k)
        }),
    }
}
