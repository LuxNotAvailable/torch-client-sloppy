pub(crate) const USER_AGENT: &str = concat!("torch-client/", env!("CARGO_PKG_VERSION"));

pub(crate) enum Fetched {
    Body(Vec<u8>),
    Cancelled,
}

mod imp {
    use super::{Fetched, USER_AGENT};
    use std::io::Read;
    use std::sync::OnceLock;

    const CHUNK: usize = 64 * 1024;

    fn client() -> &'static reqwest::blocking::Client {
        static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
        CLIENT.get_or_init(|| {
            crate::install_crypto_provider();
            let builder = reqwest::blocking::Client::builder().user_agent(USER_AGENT);
            #[cfg(target_os = "android")]
            let builder = match crate::android_tls_config() {
                Some(tls) => builder.tls_backend_preconfigured(tls),
                None => builder,
            };
            builder
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new())
        })
    }

    pub(super) async fn get_string(url: &str) -> Result<String, String> {
        match client()
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
        {
            Ok(text) => Ok(text),
            Err(e) => {
                crate::log_warn!("net", "GET {url} failed: {e:?}");
                Err(e.to_string())
            }
        }
    }

    pub(super) async fn get_bytes(
        url: &str,
        expected: u64,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<Fetched, String> {
        let mut resp = client()
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        let total = if expected > 0 {
            expected
        } else {
            resp.content_length().unwrap_or(0)
        };

        let mut out = Vec::with_capacity(total as usize);
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            if !progress(out.len() as u64, total) {
                return Ok(Fetched::Cancelled);
            }
        }
        Ok(Fetched::Body(out))
    }

    pub(super) fn get_bytes_blocking(url: &str) -> Result<Vec<u8>, String> {
        client()
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
            .map(|b| b.to_vec())
            .map_err(|e| e.to_string())
    }

    #[cfg(explore_modrinth)]
    pub(super) fn get_as_blocking(url: &str, user_agent: &str) -> Result<Vec<u8>, String> {
        client()
            .get(url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
            .map(|b| b.to_vec())
            .map_err(|e| e.to_string())
    }
}

pub(crate) async fn get_string(url: &str) -> Result<String, String> {
    imp::get_string(url).await
}

pub(crate) async fn get_bytes(
    url: &str,
    expected: u64,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Fetched, String> {
    imp::get_bytes(url, expected, progress).await
}

pub(crate) fn get_bytes_blocking(url: &str) -> Result<Vec<u8>, String> {
    imp::get_bytes_blocking(url)
}

#[cfg(explore_modrinth)]
pub(crate) fn get_as_blocking(url: &str, user_agent: &str) -> Result<Vec<u8>, String> {
    imp::get_as_blocking(url, user_agent)
}
