use super::*;

fn screen(uuid: &str, x: f32, y: f32, width: f32, height: f32) -> Screen {
    Screen {
        uuid: Some(uuid.into()),
        bounds: Bounds::new(point(px(x), px(y)), size(px(width), px(height))),
    }
}

fn screens() -> Vec<Screen> {
    // A 1920x1080 primary and a 2560x1440 secondary at 150%, left of it.
    vec![
        screen("primary", 0., 0., 1920., 1080.),
        screen("left", -1706.67, 0., 1706.67, 960.),
    ]
}

fn saved(x: i32, y: i32, display: Option<&str>) -> WindowState {
    WindowState {
        x,
        y,
        width: 1000,
        height: 700,
        maximized: false,
        display: display.map(str::to_owned),
    }
}

fn origin(placement: &Placement) -> (f32, f32) {
    let origin = placement.bounds.origin;
    (f32::from(origin.x), f32::from(origin.y))
}

#[test]
fn restores_a_primary_display_record_where_it_was() {
    let screens = screens();
    for display in [None, Some("primary")] {
        let state = saved(120, 80, display);
        let placement = place(Some(&state), &screens, 0).unwrap();
        assert_eq!(placement.screen, 0);
        assert_eq!(origin(&placement), (120., 80.));
        assert_eq!(
            placement.bounds.size,
            size(px(1000.), px(700.))
        );
        let display = state.display.as_ref().map(|_| &screens[0]);
        assert_eq!(
            record(placement.bounds, false, display),
            Some(state),
            "a saved record round-trips"
        );
    }
}

#[test]
fn restores_relative_to_a_secondary_display() {
    let screens = screens();
    let state = saved(100, 50, Some("left"));
    let placement = place(Some(&state), &screens, 0).unwrap();
    assert_eq!(placement.screen, 1);
    let (x, y) = origin(&placement);
    assert!((x - (-1606.67)).abs() < 0.01 && y == 50.);
    assert_eq!(record(placement.bounds, false, Some(&screens[1])), Some(state));
}

#[test]
fn a_missing_display_falls_back_to_the_primary_and_stays_on_screen() {
    let screens = screens();
    let state = WindowState {
        maximized: true,
        ..saved(1500, 900, Some("unplugged"))
    };
    let placement = place(Some(&state), &screens, 0).unwrap();
    assert_eq!(placement.screen, 0);
    assert_eq!(origin(&placement), (920., 380.));
    assert!(placement.maximized);
    let offscreen = saved(-5000, -300, None);
    assert_eq!(origin(&place(Some(&offscreen), &screens, 0).unwrap()), (0., 0.));
}

#[test]
fn shrinks_to_a_smaller_display_but_not_below_the_minimum() {
    let small = vec![screen("small", 0., 0., 1280., 720.)];
    let state = WindowState {
        width: 1800,
        height: 1000,
        ..saved(10, 10, Some("small"))
    };
    let placement = place(Some(&state), &small, 0).unwrap();
    assert_eq!(placement.bounds.size, size(px(1280.), px(720.)));
    assert_eq!(origin(&placement), (0., 0.));
    let tiny = vec![screen("tiny", 0., 0., 640., 480.)];
    assert_eq!(place(Some(&state), &tiny, 0).unwrap().bounds.size, min_size());
}

#[test]
fn without_a_record_opens_the_default_size_centered_on_the_primary() {
    let placement = place(None, &screens(), 0).unwrap();
    let (width, height) = client_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    assert_eq!(placement.screen, 0);
    assert_eq!(
        placement.bounds.size,
        size(px(width as f32), px(height as f32))
    );
    assert_eq!(
        origin(&placement),
        ((1920 - width) as f32 / 2., (1080 - height) as f32 / 2.)
    );
    assert!(!placement.maximized);
    assert_eq!(place(None, &[], 0), None);
}

#[test]
fn imported_electron_bounds_restore_on_the_primary_display() {
    let state = WindowState::parse_electron(
        br#"{"x":100,"y":50,"width":1220,"height":820,"maximized":false}"#,
    )
    .unwrap();
    let placement = place(Some(&state), &screens(), 0).unwrap();
    assert_eq!(placement.screen, 0);
    // Windows stores client bounds: Electron's outer bounds minus the frame.
    let expected = if cfg!(windows) {
        (108., 69., 1204., 781.)
    } else {
        (100., 50., 1220., 820.)
    };
    let (x, y) = origin(&placement);
    let bounds = placement.bounds.size;
    assert_eq!(
        (x, y, f32::from(bounds.width), f32::from(bounds.height)),
        expected
    );
}
