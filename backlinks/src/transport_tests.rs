use super::*;
use reqwest::dns::Resolve;
use std::task::{Context, Poll, Waker};

#[test]
fn b01_production_resolver_returns_only_selected_socket_without_fallback() {
    let selected = "1.1.1.1:443".parse().unwrap();
    let resolver = SelectedAddress {
        host: "example.com".into(),
        socket: selected,
    };
    let mut context = Context::from_waker(Waker::noop());
    let mut result = resolver.resolve("example.com".parse().unwrap());
    let Poll::Ready(Ok(addresses)) = result.as_mut().poll(&mut context) else {
        panic!("selected name was not resolved");
    };
    assert_eq!(addresses.collect::<Vec<_>>(), vec![selected]);
    let mut result = resolver.resolve("other.example".parse().unwrap());
    assert!(matches!(
        result.as_mut().poll(&mut context),
        Poll::Ready(Err(_))
    ));
    let t = PinnedReqwestTransport::new(1).unwrap();
    let client = t.build_client("example.com", selected).unwrap();
    let request = client.get("https://example.com/").build().unwrap();
    assert_eq!(request.url().host_str(), Some("example.com"));
}

#[test]
fn b01_stream_read_stops_at_cap_plus_one() {
    let data = vec![b'x'; 100_000];
    let mut reader = io::Cursor::new(data);
    assert_eq!(
        read_capped(&mut reader, 100).unwrap_err().kind(),
        io::ErrorKind::FileTooLarge
    );
    assert_eq!(reader.position(), 101);
    assert_eq!(
        read_capped(&mut io::Cursor::new(b"exact"), 5).unwrap(),
        b"exact"
    );
    assert!(read_capped(&mut io::Cursor::new(b""), 0)
        .unwrap()
        .is_empty());
}

#[test]
fn b01_transport_rejects_private_pins_before_socket_creation() {
    let t = PinnedReqwestTransport::new(1).unwrap();
    for ip in ["127.0.0.1", "10.0.0.1", "169.254.169.254", "::1"] {
        assert!(t
            .get(
                "https",
                "example.com",
                443,
                ip.parse().unwrap(),
                "/",
                "fixture"
            )
            .is_err());
    }
}
