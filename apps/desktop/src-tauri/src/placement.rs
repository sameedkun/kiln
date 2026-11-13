//! Where the tray popup goes ([ADR 0054] §4): against the tray icon, inside the work area of
//! the icon's monitor, whichever edge the taskbar is on. Physical pixels throughout, so
//! scaled and secondary monitors need nothing special.
//!
//! [ADR 0054]: ../../../../docs/adr/0054-desktop-app-shell.md

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Space between the popup and the taskbar or the screen's edge, as Windows' own flyouts.
pub const GAP: i32 = 12;

fn len(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

impl Rect {
    fn right(self) -> i32 {
        self.x.saturating_add(len(self.width))
    }

    fn bottom(self) -> i32 {
        self.y.saturating_add(len(self.height))
    }

    fn center_x(self) -> i32 {
        self.x.saturating_add(len(self.width) / 2)
    }

    fn center_y(self) -> i32 {
        self.y.saturating_add(len(self.height) / 2)
    }
}

/// Keeps `start..start+size` inside `low..high`, with the gap on both sides; at `low` if it
/// can't fit.
fn clamp(start: i32, size: i32, low: i32, high: i32) -> i32 {
    start.min(high - size - GAP).max(low + GAP)
}

/// The popup's top-left corner for a popup of `size` (width, height), given the tray icon's
/// rectangle and the work area (the monitor without the taskbar) of the icon's monitor.
#[must_use]
pub fn popup_position(icon: Rect, work: Rect, size: (u32, u32)) -> (i32, i32) {
    let (width, height) = (len(size.0), len(size.1));
    let across = clamp(icon.center_x() - width / 2, width, work.x, work.right());
    let along = clamp(icon.center_y() - height / 2, height, work.y, work.bottom());
    if icon.y >= work.bottom() {
        // Taskbar at the bottom, the usual place: above it.
        (across, work.bottom() - height - GAP)
    } else if icon.bottom() <= work.y {
        // At the top: below it.
        (across, work.y + GAP)
    } else if icon.x >= work.right() {
        // On the right: to its left.
        (work.right() - width - GAP, along)
    } else if icon.right() <= work.x {
        // On the left: to its right.
        (work.x + GAP, along)
    } else {
        // The icon is inside the work area (an auto-hiding taskbar, the overflow flyout):
        // above it if there is room, else below, always on the screen.
        let above = icon.y - height - GAP;
        let y = if above >= work.y + GAP {
            above
        } else {
            icon.bottom() + GAP
        };
        (across, clamp(y, height, work.y, work.bottom()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POPUP: (u32, u32) = (340, 560);

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn above_a_bottom_taskbar_centred_on_the_icon() {
        let work = rect(0, 0, 1920, 1032);
        assert_eq!(
            popup_position(rect(1700, 1044, 24, 24), work, POPUP),
            (1542, 460)
        );
    }

    #[test]
    fn never_past_the_screens_edge() {
        let work = rect(0, 0, 1920, 1032);
        // An icon in the far corner: the popup's right edge stops a gap from the screen's.
        assert_eq!(
            popup_position(rect(1896, 1044, 24, 24), work, POPUP),
            (1568, 460)
        );
        // And at the far left.
        assert_eq!(popup_position(rect(2, 1044, 24, 24), work, POPUP).0, GAP);
    }

    #[test]
    fn below_a_top_taskbar() {
        let work = rect(0, 48, 1920, 1032);
        assert_eq!(
            popup_position(rect(1700, 12, 24, 24), work, POPUP),
            (1542, 60)
        );
    }

    #[test]
    fn beside_a_left_or_right_taskbar() {
        let right = rect(0, 0, 1872, 1080);
        assert_eq!(
            popup_position(rect(1884, 900, 24, 24), right, POPUP),
            (1520, 508)
        );
        let left = rect(48, 0, 1872, 1080);
        assert_eq!(popup_position(rect(12, 300, 24, 24), left, POPUP), (60, 32));
    }

    #[test]
    fn on_a_secondary_monitor_left_of_the_main_one() {
        // A 2560 x 1440 monitor at 150% to the left: physical pixels, negative x.
        let work = rect(-2560, 0, 2560, 1392);
        let (x, y) = popup_position(rect(-300, 1400, 36, 36), work, (510, 840));
        assert_eq!((x, y), (-537, 540));
        assert!(x + 510 <= -GAP);
    }

    #[test]
    fn an_icon_inside_the_work_area_gets_the_popup_above_it() {
        let work = rect(0, 0, 1920, 1080);
        assert_eq!(
            popup_position(rect(1700, 1040, 24, 24), work, POPUP),
            (1542, 468)
        );
        // Near the top, there is no room above: below it.
        assert_eq!(popup_position(rect(1700, 40, 24, 24), work, POPUP).1, 76);
    }
}
