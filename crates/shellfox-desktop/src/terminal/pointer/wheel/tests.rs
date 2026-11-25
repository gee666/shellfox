use super::{Steps, Wheel};
use gpui::{Modifiers, ScrollDelta, point, px};
use shellfox_ghostty_vt::Terminal;

const LINE: f32 = 17.;

fn lines(x: f32, y: f32) -> ScrollDelta {
    ScrollDelta::Lines(point(x, y))
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Default::default()
    }
}

fn top_line(terminal: &mut Terminal) -> String {
    let screen = terminal.render().unwrap();
    let row: String = screen.cells[..usize::from(screen.cols)]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect();
    row.trim_end().to_owned()
}

/// One Windows wheel notch is three lines (GPUI scales WHEEL_DELTA by the
/// system setting); Shift+wheel arrives as the same amount on the x axis.
#[test]
fn wheel_notches_scroll_the_viewport_through_history() {
    let mut terminal = Terminal::new(40, 10).unwrap();
    for line in 1..=100 {
        terminal
            .feed(format!("line {line}\r\n").as_bytes())
            .unwrap();
    }
    assert_eq!(top_line(&mut terminal), "line 92");
    let mut wheel = Wheel::default();
    for (delta, mods, top) in [
        (lines(0., 3.), Modifiers::default(), "line 89"),
        (lines(3., 0.), shift(), "line 86"),
        (lines(0., -3.), Modifiers::default(), "line 89"),
    ] {
        let steps = wheel.steps(delta, LINE, mods, false);
        terminal.scroll(-(steps.y as isize)).unwrap();
        assert_eq!(top_line(&mut terminal), top);
    }
}

#[test]
fn local_scrolling_ignores_horizontal_and_speeds_up_with_alt() {
    let mut wheel = Wheel::default();
    assert_eq!(
        wheel.steps(lines(4., 0.), LINE, Modifiers::default(), false),
        Steps { x: 0, y: 0 }
    );
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        wheel.steps(lines(0., -1.), LINE, alt, false),
        Steps { x: 0, y: -5 }
    );
}

/// Precision touchpads and smooth-scrolling mice send fractions of a line.
#[test]
fn small_pixel_deltas_accumulate_into_lines() {
    let mut wheel = Wheel::default();
    let nudge = ScrollDelta::Pixels(point(px(0.), px(LINE / 4.)));
    let steps: Vec<i32> = (0..8)
        .map(|_| wheel.steps(nudge, LINE, Modifiers::default(), false).y)
        .collect();
    assert_eq!(steps.iter().sum::<i32>(), 2);
    assert!(steps.iter().all(|step| (0..=1).contains(step)));
}

#[test]
fn reported_wheel_keeps_both_axes_and_refunds_unsent_steps() {
    let mut wheel = Wheel::default();
    assert_eq!(
        wheel.steps(lines(-2., 3.), LINE, shift(), true),
        Steps { x: -2, y: 3 }
    );
    wheel.refund(true, 2);
    assert_eq!(
        wheel.steps(lines(0., 0.), LINE, Modifiers::default(), true),
        Steps { x: 0, y: 2 }
    );
}
