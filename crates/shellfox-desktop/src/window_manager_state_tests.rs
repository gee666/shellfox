use super::*;
fn primary() -> Monitor {
    Monitor {
        id: "primary".into(),
        work_area: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        physical_origin: (0, 0),
        scale_factor: 1.,
    }
}
fn saved() -> WindowState {
    WindowState {
        x: 80,
        y: 60,
        width: 1220,
        height: 820,
        maximized: false,
    }
}
#[test]
fn exact_electron_defaults_schema_and_title_strip_thresholds() {
    let primary = primary();
    let restore = |saved| restore_bounds(saved, &[], &primary).unwrap();
    assert_eq!(
        restore(None),
        RestoredBounds {
            origin: None,
            width: 1220,
            height: 820,
            maximized: false
        }
    );
    assert_eq!(restore(Some(saved())).origin, Some((80, 60)));
    for state in [
        WindowState {
            width: 849,
            ..saved()
        },
        WindowState {
            height: 599,
            ..saved()
        },
        WindowState {
            x: 100_001,
            ..saved()
        },
        WindowState {
            width: 20_001,
            ..saved()
        },
    ] {
        assert!(!state.valid());
        assert_eq!(restore(Some(state)), restore(None));
    }
    assert_eq!(
        restore(Some(WindowState {
            x: 1820,
            y: -39,
            ..saved()
        }))
        .origin,
        Some((1820, -39))
    );
    assert_eq!(
        restore(Some(WindowState { x: 1821, ..saved() })).origin,
        None
    );
    assert_eq!(
        restore(Some(WindowState { y: -40, ..saved() })).origin,
        None
    );
    assert_eq!(
        restore(Some(WindowState { y: 1080, ..saved() })).origin,
        None
    );
}
#[test]
fn wayland_restores_size_and_maximize_without_importing_an_x11_origin() {
    let state = WindowState { x: -15_000, y: -2000, width: 1220, height: 820, maximized: true };
    assert_eq!(restore_size_only(Some(state), &primary()).unwrap(), RestoredBounds {
        origin: None, width: 1220, height: 820, maximized: true,
    });
}

#[test]
fn wayland_clamps_default_and_saved_bounds_to_the_logical_output() {
    let monitor = Monitor {
        work_area: Rect { x: 0, y: 0, width: 1200, height: 760 },
        ..primary()
    };
    let restored = RestoredBounds {
        origin: None, width: 1200, height: 760, maximized: false,
    };
    assert_eq!(restore_size_only(None, &monitor).unwrap(), restored);
    assert_eq!(restore_size_only(Some(saved()), &monitor).unwrap(), restored);
}

#[test]
fn mixed_dpi_restore_clamps_detached_bounds_in_primary_work_area_dips() {
    let primary = Monitor {
        work_area: Rect { x: 0, y: 0, width: 1536, height: 832 },
        scale_factor: 1.25,
        ..primary()
    };
    let left = Monitor {
        id: "left-150-percent".into(),
        work_area: Rect { x: -1707, y: 25, width: 1707, height: 910 },
        physical_origin: (-2560, 38),
        scale_factor: 1.5,
    };
    let saved = WindowState { x: -1501, y: 62, width: 1600, height: 900, maximized: true };
    assert_eq!(restore_bounds(Some(saved), std::slice::from_ref(&left), &primary).unwrap(),
        RestoredBounds { origin: Some((-1501, 62)), width: 1600, height: 900, maximized: true });
    assert_eq!(restore_bounds(Some(saved), &[], &primary).unwrap(),
        RestoredBounds { origin: None, width: 1536, height: 832, maximized: true });
}

#[test]
fn removed_monitor_recenters_and_clamps_size_but_preserves_maximize_and_negative_display() {
    let primary = primary();
    let state = WindowState {
        x: -2500,
        y: 20,
        width: 2400,
        height: 1500,
        maximized: true,
    };
    let restored = restore_bounds(Some(state), &[], &primary).unwrap();
    assert_eq!(
        restored,
        RestoredBounds {
            origin: None,
            width: 1920,
            height: 1080,
            maximized: true
        }
    );
    let left = Monitor {
        id: "left".into(),
        work_area: Rect {
            x: -2560,
            y: 0,
            width: 2560,
            height: 1440,
        },
        physical_origin: (-3840, 0),
        scale_factor: 1.5,
    };
    assert_eq!(
        restore_bounds(Some(state), &[left], &primary)
            .unwrap()
            .origin,
        Some((-2500, 20))
    );
}
#[test]
fn physical_normal_bounds_round_trip_across_dpi_and_monitor_origins() {
    let monitor = Monitor {
        id: "left".into(),
        work_area: Rect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        physical_origin: (-2880, 0),
        scale_factor: 1.5,
    };
    let logical = Rect {
        x: -1800,
        y: 60,
        width: 1000,
        height: 700,
    };
    let physical = monitor.to_physical(logical).unwrap();
    assert_eq!(
        physical,
        Rect {
            x: -2700,
            y: 90,
            width: 1500,
            height: 1050
        }
    );
    let capture = Capture {
        destroyed: false,
        minimized: false,
        maximized: true,
        fullscreen: false,
        current_pixels: Rect {
            x: -2880,
            y: 0,
            width: 2880,
            height: 1620,
        },
        normal_pixels: physical,
    };
    let saved = capture_physical(capture, &monitor).unwrap().unwrap();
    assert_eq!(
        saved,
        WindowState {
            x: logical.x,
            y: logical.y,
            width: logical.width,
            height: logical.height,
            maximized: true
        }
    );
    let changed_dpi = Monitor {
        scale_factor: 2.,
        ..monitor.clone()
    };
    assert_eq!(changed_dpi.to_physical(logical).unwrap().width, 2000);
    assert!(
        capture_physical(
            Capture {
                minimized: true,
                ..capture
            },
            &monitor
        )
        .unwrap()
        .is_none()
    );
    assert!(
        capture_physical(
            Capture {
                destroyed: true,
                ..capture
            },
            &monitor
        )
        .unwrap()
        .is_none()
    );
    assert!(
        capture_physical(
            Capture {
                maximized: false,
                fullscreen: true,
                ..capture
            },
            &monitor
        )
        .unwrap()
        .unwrap()
        .maximized
    );
    for scale_factor in [0., -1., f64::NAN, f64::INFINITY, 9.] {
        let invalid = Monitor {
            scale_factor,
            ..monitor.clone()
        };
        assert!(!invalid.valid());
        assert!(invalid.to_physical(logical).is_err());
    }
}
#[test]
fn state_keys_separate_devices_and_windows_and_reject_paths() {
    let a = WindowStateKey::new("device-a".into(), "window-1".into()).unwrap();
    let b = WindowStateKey::new("device-a".into(), "window-2".into()).unwrap();
    let c = WindowStateKey::new("device-b".into(), "window-1".into()).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.device(), "device-a");
    assert_eq!(a.window(), "window-1");
    for key in ["", "../private", "a/b", "a\\b"] {
        assert!(WindowStateKey::new(key.into(), "window".into()).is_err());
    }
}
#[test]
fn moves_debounce_400_ms_and_close_cancels_timer() {
    let start = Instant::now();
    let mut tracker = SaveDebounce::default();
    tracker.changed(start);
    tracker.changed(start + Duration::from_millis(100));
    assert!(!tracker.take_due(start + Duration::from_millis(499)));
    assert!(tracker.take_due(start + Duration::from_millis(500)));
    assert!(!tracker.take_due(start + Duration::from_millis(501)));
    tracker.changed(start);
    tracker.flush();
    assert!(!tracker.take_due(start + Duration::from_secs(1)));
}
#[test]
fn native_window_options_disable_macos_tabbing_without_changing_style() {
    let options = gpui::WindowOptions {
        tabbing_identifier: Some("unsafe-native-tabs".into()),
        titlebar: None,
        ..Default::default()
    };
    let restored = restore_bounds(
        Some(WindowState {
            x: 5000,
            maximized: true,
            ..saved()
        }),
        &[],
        &primary(),
    )
    .unwrap();
    let options = super::super::adapter::window_options(options, restored, &primary()).unwrap();
    assert!(options.tabbing_identifier.is_none());
    assert!(options.titlebar.is_none());
    let Some(gpui::WindowBounds::Maximized(bounds)) = options.window_bounds else {
        panic!("must restore maximized")
    };
    assert!(f32::from(bounds.origin.x) >= 0.);
    assert!(f32::from(bounds.origin.y) >= 0.);
}
