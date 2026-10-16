use backlink_opportunities_detector::transport::{HttpTransport, PinnedReqwestTransport};

fn main() -> anyhow::Result<()> {
    let transport = PinnedReqwestTransport::with_body_limit(5, 65536)?;
    let response = transport.get(
        "https",
        "example.com",
        443,
        "1.1.1.1".parse()?,
        "/",
        "backlink-pin-probe/1",
    )?;
    println!("Production transport verified peer 1.1.1.1:443 with original TLS hostname example.com; HTTP {}; body bytes {}", response.status, response.body.len());
    Ok(())
}
