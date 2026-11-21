//! The private channel between the MCP connector and the TagRex window (#828).
//!
//! One request per connection: the connector writes one frame, the window
//! answers with one frame and closes. A frame is a 4-byte big-endian length and
//! that many bytes of JSON. The secret travels in every request, so a revoked
//! client stops working on its next call rather than at its next start.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when either side's frames change shape. A connector from another
/// release is told to restart instead of being misread.
pub const PROTOCOL: u32 = 1;
/// The largest request the window reads: the arguments of one tool call.
pub const MAX_REQUEST: usize = 1 << 20;
/// The largest answer either side accepts. A page of tracks is far below it;
/// the limit only stops a runaway result from filling memory.
pub const MAX_RESPONSE: usize = 32 << 20;

/// One tool call, as the connector sends it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub v: u32,
    pub client: String,
    pub secret: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

/// The window's answer.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok {
        result: Value,
    },
    Error {
        message: String,
    },
    /// The client is not paired (any more), or its secret does not match.
    Unauthorized,
}

pub fn write_frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

pub fn read_frame(reader: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    reader.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes is over the {max}-byte limit"),
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(body)
}

pub async fn read_frame_async(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    max: usize,
) -> io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut len = [0u8; 4];
    reader.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes is over the {max}-byte limit"),
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(body)
}

pub async fn write_frame_async(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    body: &[u8],
) -> io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_reads_back_as_written() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, b"{\"a\":1}").unwrap();
        assert_eq!(&buffer[..4], &[0, 0, 0, 7]);
        let body = read_frame(&mut buffer.as_slice(), 64).unwrap();
        assert_eq!(body, b"{\"a\":1}");
    }

    #[test]
    fn a_frame_over_the_limit_is_refused_before_it_is_read() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &[b'x'; 100]).unwrap();
        let error = read_frame(&mut buffer.as_slice(), 99).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_truncated_frame_is_an_error() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, b"hello").unwrap();
        buffer.pop();
        assert!(read_frame(&mut buffer.as_slice(), 64).is_err());
    }

    #[test]
    fn responses_are_tagged_by_status() {
        let text = serde_json::to_string(&Response::Unauthorized).unwrap();
        assert_eq!(text, r#"{"status":"unauthorized"}"#);
        let ok: Response = serde_json::from_str(r#"{"status":"ok","result":{"n":2}}"#).unwrap();
        assert_eq!(
            ok,
            Response::Ok {
                result: serde_json::json!({"n": 2})
            }
        );
    }
}
