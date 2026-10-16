use crate::fetch::Pacer;
use crate::transport::{HttpTransport, RawResponse};
use std::{io, net::IpAddr, sync::Mutex};

pub struct PacedTransport<'a> {
    inner: &'a dyn HttpTransport,
    pacer: Mutex<Pacer>,
}

impl<'a> PacedTransport<'a> {
    pub fn new(inner: &'a dyn HttpTransport, delay_ms: u64) -> Self {
        Self {
            inner,
            pacer: Mutex::new(Pacer::new(delay_ms)),
        }
    }
}

impl HttpTransport for PacedTransport<'_> {
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        self.inner.resolve(host)
    }

    fn get(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        ip: IpAddr,
        path: &str,
        ua: &str,
    ) -> io::Result<RawResponse> {
        self.pacer
            .lock()
            .map_err(|_| io::Error::other("request pacer unavailable"))?
            .wait(&format!("{scheme}://{host}:{port}"));
        self.inner.get(scheme, host, port, ip, path, ua)
    }
}
