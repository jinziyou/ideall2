//! Authenticated, bounded HTTP transport for partitioned synchronization.

use std::{io::Read as _, time::Duration};

use ideall_sync::{SyncGenerationPart, SyncManifest, SyncTransport, TransportError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use url::Url;

const MANIFEST_RESPONSE_LIMIT: u64 = 32 * 1024;
const PART_RESPONSE_LIMIT: u64 = 6 * 1024 * 1024;
const TOKEN_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Error)]
pub enum HttpTransportConfigError {
    #[error("sync server must be an http(s) base URL without credentials, query, or fragment")]
    InvalidServer,
    #[error("sync bearer token is empty or too large")]
    InvalidToken,
}

pub struct HttpSyncTransport {
    agent: ureq::Agent,
    api_root: Url,
    authorization: String,
}

impl HttpSyncTransport {
    /// Creates a client from the server origin or deployment base. The adapter
    /// appends `v2/app/` while preserving an existing path prefix.
    pub fn new(server_base: &str, bearer_token: &str) -> Result<Self, HttpTransportConfigError> {
        let mut api_root = Url::parse(server_base)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .filter(|url| url.host_str().is_some())
            .filter(|url| url.username().is_empty() && url.password().is_none())
            .filter(|url| url.query().is_none() && url.fragment().is_none())
            .ok_or(HttpTransportConfigError::InvalidServer)?;
        {
            let mut path = api_root
                .path_segments_mut()
                .map_err(|_| HttpTransportConfigError::InvalidServer)?;
            path.pop_if_empty().extend(["v2", "app"]);
        }
        if bearer_token.is_empty()
            || bearer_token.len() > TOKEN_MAX_BYTES
            || !bearer_token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(HttpTransportConfigError::InvalidToken);
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Ok(Self {
            agent: config.into(),
            api_root,
            authorization: format!("Bearer {bearer_token}"),
        })
    }

    fn endpoint(&self, segments: &[&str]) -> Result<Url, TransportError> {
        let mut url = self.api_root.clone();
        url.path_segments_mut()
            .map_err(|_| TransportError::new(None, "invalid synchronization server URL"))?
            .extend(segments);
        Ok(url)
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        url: Url,
        response_limit: u64,
    ) -> Result<T, TransportError> {
        let response = self
            .agent
            .get(url.as_str())
            .header("Authorization", &self.authorization)
            .header("Accept", "application/json")
            .call()
            .map_err(map_ureq_error)?;
        decode_envelope(response, response_limit)
    }

    fn put_json<B: Serialize, T: DeserializeOwned>(
        &self,
        url: Url,
        body: &B,
        response_limit: u64,
    ) -> Result<T, TransportError> {
        let response = self
            .agent
            .put(url.as_str())
            .header("Authorization", &self.authorization)
            .header("Accept", "application/json")
            .send_json(body)
            .map_err(map_ureq_error)?;
        decode_envelope(response, response_limit)
    }
}

impl SyncTransport for HttpSyncTransport {
    fn get_manifest(&mut self, storage_id: &str) -> Result<Option<SyncManifest>, TransportError> {
        let url = self.endpoint(&["sync", storage_id, "manifest"])?;
        match self.get_json(url, MANIFEST_RESPONSE_LIMIT) {
            Ok(manifest) => Ok(Some(manifest)),
            Err(error) if error.status == Some(404) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn get_part(
        &mut self,
        storage_id: &str,
        generation: &str,
        part_index: usize,
    ) -> Result<SyncGenerationPart, TransportError> {
        self.get_json(
            self.endpoint(&[
                "sync",
                storage_id,
                "generations",
                generation,
                "parts",
                &part_index.to_string(),
            ])?,
            PART_RESPONSE_LIMIT,
        )
    }

    fn put_part(
        &mut self,
        storage_id: &str,
        generation: &str,
        part: &SyncGenerationPart,
    ) -> Result<(), TransportError> {
        #[derive(Serialize)]
        struct PartWrite<'a> {
            iv: &'a str,
            ciphertext: &'a str,
        }
        let _: serde::de::IgnoredAny = self.put_json(
            self.endpoint(&[
                "sync",
                storage_id,
                "generations",
                generation,
                "parts",
                &part.part_index.to_string(),
            ])?,
            &PartWrite {
                iv: &part.iv,
                ciphertext: &part.ciphertext,
            },
            MANIFEST_RESPONSE_LIMIT,
        )?;
        Ok(())
    }

    fn commit_manifest(
        &mut self,
        storage_id: &str,
        generation: &str,
        part_count: usize,
        expected_version: u64,
    ) -> Result<SyncManifest, TransportError> {
        #[derive(Serialize)]
        struct ManifestWrite<'a> {
            generation: &'a str,
            part_count: usize,
        }
        let mut url = self.endpoint(&["sync", storage_id, "manifest"])?;
        url.query_pairs_mut()
            .append_pair("expected", &expected_version.to_string());
        self.put_json(
            url,
            &ManifestWrite {
                generation,
                part_count,
            },
            MANIFEST_RESPONSE_LIMIT,
        )
    }

    fn discard_generation(
        &mut self,
        storage_id: &str,
        generation: &str,
    ) -> Result<(), TransportError> {
        let url = self.endpoint(&["sync", storage_id, "generations", generation])?;
        match self
            .agent
            .delete(url.as_str())
            .header("Authorization", &self.authorization)
            .header("Accept", "application/json")
            .call()
        {
            Ok(_) | Err(ureq::Error::StatusCode(404)) => Ok(()),
            Err(error) => Err(map_ureq_error(error)),
        }
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

fn decode_envelope<T: DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
    limit: u64,
) -> Result<T, TransportError> {
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|_| TransportError::new(None, "failed to read synchronization response"))?;
    if body.len() as u64 > limit {
        return Err(TransportError::new(
            None,
            "synchronization response exceeded its budget",
        ));
    }
    serde_json::from_slice::<Envelope<T>>(&body)
        .map(|envelope| envelope.data)
        .map_err(|_| TransportError::new(None, "invalid synchronization response"))
}

fn map_ureq_error(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::StatusCode(status) => TransportError::new(
            Some(status),
            format!("synchronization server returned {status}"),
        ),
        _ => TransportError::new(None, "synchronization network request failed"),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead as _, BufReader, Write as _},
        net::{TcpListener, TcpStream},
        thread,
    };

    use super::*;

    fn serve_once(
        response_status: &str,
        response_body: &str,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = response_status.to_owned();
        let body = response_body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            request
        });
        (format!("http://{address}/prefix/"), handle)
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            request.push_str(&line);
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        request
    }

    #[test]
    fn manifest_request_is_authenticated_and_preserves_the_deployment_prefix() {
        let body = r#"{"data":{"generation":"g","part_count":1,"total_ciphertext_chars":4,"parts_sha256":"aa","version":7,"updated_at_ms":9}}"#;
        let (base, server) = serve_once("200 OK", body);
        let mut transport = HttpSyncTransport::new(&base, "secret-token").unwrap();
        let manifest = transport.get_manifest("storage/id").unwrap().unwrap();
        assert_eq!(manifest.version, 7);
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /prefix/v2/app/sync/storage%2Fid/manifest HTTP/1.1"));
        assert!(request.contains("authorization: Bearer secret-token\r\n"));
    }

    #[test]
    fn missing_manifest_is_an_empty_remote_snapshot() {
        let (base, server) = serve_once("404 Not Found", "");
        let mut transport = HttpSyncTransport::new(&base, "token").unwrap();
        assert_eq!(transport.get_manifest("storage").unwrap(), None);
        server.join().unwrap();
    }

    #[test]
    fn rejects_credential_bearing_urls_and_header_injection() {
        assert!(matches!(
            HttpSyncTransport::new("https://user@example.com", "token"),
            Err(HttpTransportConfigError::InvalidServer)
        ));
        assert!(matches!(
            HttpSyncTransport::new("https://example.com", "bad\r\ntoken"),
            Err(HttpTransportConfigError::InvalidToken)
        ));
    }
}
