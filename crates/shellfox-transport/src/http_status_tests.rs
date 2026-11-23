use super::*;
#[tokio::test]
async fn status_reads_return_the_actual_status_and_opaque_bounded_body() {
    for status in [200_u16, 302, 403, 404, 409, 503] {
        let body: &[u8] = if status == 200 { b"{}" } else { b"opaque\xffbody" };
        let mut raw = format!("HTTP/1.1 {status} Actual\r\nContent-Length: {}\r\n\r\n",body.len()).into_bytes();
        raw.extend_from_slice(body);
        let response = read_response_with_status(&mut BufReader::new(raw.as_slice()), 64, false).await.unwrap();
        assert_eq!(response.status,status);
        assert_eq!(response.body,body);
        if status != 200 {
            assert_eq!(read_response(&mut BufReader::new(raw.as_slice()),0).await,Err(TransportError::Rejected{status}));
        }
    }
    assert_eq!(read_response_with_status(&mut BufReader::new(b"HTTP/1.1 403 Actual\r\nContent-Length: 3\r\n\r\nabc".as_slice()),2,false).await,Err(TransportError::TooLarge));
    assert_eq!(read_response_with_status(&mut BufReader::new(b"HTTP/1.1 200 Actual\r\nContent-Length: 3\r\n\r\nabc".as_slice()),8,false).await,Err(TransportError::InvalidResponse));
}
