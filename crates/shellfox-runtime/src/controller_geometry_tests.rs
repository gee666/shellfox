use super::*;
#[test]
fn stamped_mouse_rejects_stale_geometry_and_ignored_real_encoder_output_is_zero_not_fake_acceptance(
) {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let snapshot = recover(&mut owner, lease);
    ack(&mut owner, lease, &snapshot).unwrap();
    let mut old = stamp(&snapshot);
    old.cols += 1;
    assert_eq!(
        owner
            .command(
                RequestId(30),
                Command::MouseStamped {
                    lease,
                    event: mouse(),
                    geometry: shellfox_ghostty_vt::MouseGeometry {
                        cell_width: 8,
                        cell_height: 16
                    },
                    stamp: old
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleLayout
    );
    assert!(matches!(
        owner
            .command(
                RequestId(31),
                Command::MouseStamped {
                    lease,
                    event: mouse(),
                    geometry: shellfox_ghostty_vt::MouseGeometry {
                        cell_width: 8,
                        cell_height: 16
                    },
                    stamp: stamp(&snapshot)
                }
            )
            .unwrap(),
        Response::Input(InputReceipt {
            accepted_bytes: 0,
            ..
        })
    ));
}
