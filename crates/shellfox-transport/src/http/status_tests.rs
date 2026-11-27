use super::*;
#[tokio::test]
async fn status_reads_return_the_actual_status_and_opaque_bounded_body() {
    for status in [200_u16, 302, 403, 404, 409, 503] {
        let body: &[u8] = if status == 200 {
            b"{}"
        } else {
            b"opaque\xffbody"
        };
        let mut raw = format!(
            "HTTP/1.1 {status} Actual\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        raw.extend_from_slice(body);
        let response = read_response_with_status(&mut BufReader::new(raw.as_slice()), 64, false)
            .await
            .unwrap();
        assert_eq!(response.status, status);
        assert_eq!(response.body, body);
        if status != 200 {
            assert_eq!(
                read_response(&mut BufReader::new(raw.as_slice()), 0).await,
                Err(TransportError::Rejected { status })
            );
        }
    }
    assert_eq!(
        read_response_with_status(
            &mut BufReader::new(b"HTTP/1.1 403 Actual\r\nContent-Length: 3\r\n\r\nabc".as_slice()),
            2,
            false
        )
        .await,
        Err(TransportError::TooLarge)
    );
    assert_eq!(
        read_response_with_status(
            &mut BufReader::new(b"HTTP/1.1 200 Actual\r\nContent-Length: 3\r\n\r\nabc".as_slice()),
            8,
            false
        )
        .await,
        Err(TransportError::InvalidResponse)
    );
}

#[tokio::test]
async fn the_proxy_client_closes_first_so_the_helper_port_keeps_no_time_wait() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let helper = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut next = String::new();
            reader.read_line(&mut next).await.unwrap();
            if next == "\r\n" {
                break;
            }
            head.push_str(&next);
        }
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        let mut body = vec![0; length];
        reader.read_exact(&mut body).await.unwrap();
        let mut stream = reader.into_inner();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
        // Keep-alive: the helper never closes. The client must, right after the body.
        let mut rest = [0; 1];
        let closed =
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut rest)).await;
        (head, closed.map(|read| read.unwrap()))
    });
    let response = post_with_status(address, "secret", b"{}", 64, false)
        .await
        .unwrap();
    assert_eq!(
        (response.status, response.body.as_slice()),
        (200, b"{}".as_slice())
    );
    let (head, closed) = helper.await.unwrap();
    assert!(!head.to_ascii_lowercase().contains("connection: close"));
    assert_eq!(
        closed,
        Ok(0),
        "the client did not close after the framed response"
    );
}
