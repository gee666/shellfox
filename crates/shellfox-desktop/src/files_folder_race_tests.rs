use super::{FileList, LinkOrigin, Navigation};
use shellfox_core::TabId;
use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Duration,
};

#[test]
fn delayed_loopback_terminal_directory_result_cannot_replace_the_selected_peer_tab() {
    let a: TabId = "00000000-0000-4000-8000-000000000001".parse().unwrap();
    let b: TabId = "00000000-0000-4000-8000-000000000002".parse().unwrap();
    let request = Navigation::Terminal(LinkOrigin::Peer {
        tab: a,
        navigation: 1,
        source: 10,
    });
    let Navigation::Terminal(origin) = request else {
        unreachable!()
    };
    let wait = Duration::from_secs(5);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut client = TcpStream::connect_timeout(&listener.local_addr().unwrap(), wait).unwrap();
    client.set_read_timeout(Some(wait)).unwrap();
    client.set_write_timeout(Some(wait)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    // Darwin inherits O_NONBLOCK from the listener. Only accept is nonblocking.
    server.set_nonblocking(false).unwrap();
    server.set_read_timeout(Some(wait)).unwrap();
    server.set_write_timeout(Some(wait)).unwrap();
    let (requested, received) = mpsc::channel();
    let (release, released) = mpsc::channel();

    thread::scope(|scope| {
        scope.spawn(move || {
            let mut input = String::new();
            BufReader::new(&server).read_line(&mut input).unwrap();
            assert_eq!(input.trim(), format!("{a} ."));
            requested.send(()).unwrap();
            released.recv_timeout(wait).unwrap();
            let delayed = FileList {
                path: "/guest/a/late-cwd".into(),
                parent_path: Some("/guest/a".into()),
                entries: vec![],
                truncated: false,
            };
            serde_json::to_writer(&mut server, &delayed).unwrap();
        });
        writeln!(client, "{a} .").unwrap();
        received.recv_timeout(wait).unwrap();
        assert!(origin.matches_peer(10, Some(a), Some(1)));

        let current_tab = Some(b);
        let current_navigation = Some(2);
        let mut displayed = "/guest/b".to_owned();
        release.send(()).unwrap();
        let delayed: FileList = serde_json::from_reader(client).unwrap();
        assert_eq!(delayed.path, "/guest/a/late-cwd");
        if origin.matches_peer(10, current_tab, current_navigation) {
            displayed = delayed.path;
        }
        assert_eq!(displayed, "/guest/b");
        assert!(
            !origin.matches_peer(10, Some(a), Some(3)),
            "returning to A must not revive its request"
        );
        assert!(
            !origin.matches_peer(11, Some(a), Some(1)),
            "another host must not inherit A's request"
        );
        assert!(
            LinkOrigin::Peer {
                tab: b,
                navigation: 2,
                source: 10
            }
            .matches_peer(10, current_tab, current_navigation)
        );
    });
}
