use std::{error::Error,io, fmt, env};
use bytes::{BytesMut, Bytes, buf};
use futures_util::SinkExt;
use http::{Response, Request, StatusCode, HeaderValue, response};
use serde_derive::Serialize;
use tokio::{net::{TcpListener, TcpStream}, io::{AsyncWriteExt, AsyncReadExt}};
use tokio_util::codec::{Decoder, Encoder, Framed};
use tokio_stream::StreamExt;
use serde::de;
use tokio::fs::File;

#[tokio::main]
async fn main() {
        let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let listener = TcpListener::bind(&addr).await.unwrap();
    println!("Listening on: http://{}", addr);
    loop {
        let (stream, _) = listener.accept().await.unwrap();
       tokio::spawn(async move {
        if let Err(e) = process(stream).await {
                println!("failed to process connection; error = {}", e);
            }
        });
    
}


async fn process(stream: TcpStream) -> Result<(), Box<dyn Error>> {
    let mut transport = Framed::new(stream, Http);

    while let Some(request) = transport.next().await {
        match request {
            Ok(request) => {
                let response = respond(request).await?;
                transport.send(response).await?;
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

async fn respond(req: Request<()>) -> Result<Response<Vec<u8>>, Box<dyn Error>> {
    let mut response = Response::builder();
    let mut file = File::open("G:\\web\\radio-server-rust\\a.mp3").await?;
    let mut contents = vec![];
    ;
    file.read_to_end(&mut contents).await.unwrap();
    let response:Response<Vec<u8>> = response.status(200).header("Connection", "close").header("Content-Type", "audio/mpeg")
        .body(contents)
        .unwrap();

    Ok(response)
}

struct Http;

/// Implementation of encoding an HTTP response into a `BytesMut`, basically
/// just writing out an HTTP/1.1 response.
impl Encoder<Response<Vec<u8>>> for Http {
    type Error = io::Error;

    fn encode(&mut self, item:Response<Vec<u8>>, dst: &mut BytesMut) -> io::Result<()> {
        use std::fmt::Write;

        write!(
            BytesWrite(dst),
            "\
             HTTP/1.1 {}\r\n\
             ",
            item.status(),
        )
        .unwrap();

        for (k, v) in item.headers() {
            dst.extend_from_slice(k.as_str().as_bytes());
            dst.extend_from_slice(b": ");
            dst.extend_from_slice(v.as_bytes());
            dst.extend_from_slice(b"\r\n");
        }

        dst.extend_from_slice(b"\r\n");
        dst.extend_from_slice(&item.body()[..]);

        return Ok(());

        // Right now `write!` on `Vec<u8>` goes through io::Write and is not
        // super speedy, so inline a less-crufty implementation here which
        // doesn't go through io::Error.
        struct BytesWrite<'a>(&'a mut BytesMut);

        impl fmt::Write for BytesWrite<'_> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0.extend_from_slice(s.as_bytes());
                Ok(())
            }

            fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
                fmt::write(self, args)
            }
        }
    }
}


impl Decoder for Http {
    type Item = Request<()>;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Request<()>>> {
        // TODO: we should grow this headers array if parsing fails and asks
        //       for more headers
        let mut headers = [None; 16];
        let (method, path, version, amt) = {
            let mut parsed_headers = [httparse::EMPTY_HEADER; 16];
            let mut r = httparse::Request::new(&mut parsed_headers);
            let status = r.parse(src).map_err(|e| {
                let msg = format!("failed to parse http request: {:?}", e);
                io::Error::new(io::ErrorKind::Other, msg)
            })?;

            let amt = match status {
                httparse::Status::Complete(amt) => amt,
                httparse::Status::Partial => return Ok(None),
            };

            let toslice = |a: &[u8]| {
                let start = a.as_ptr() as usize - src.as_ptr() as usize;
                assert!(start < src.len());
                (start, start + a.len())
            };

            for (i, header) in r.headers.iter().enumerate() {
                let k = toslice(header.name.as_bytes());
                let v = toslice(header.value);
                headers[i] = Some((k, v));
            }

            let method = http::Method::try_from(r.method.unwrap())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            (
                method,
                toslice(r.path.unwrap().as_bytes()),
                r.version.unwrap(),
                amt,
            )
        };
        if version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "only HTTP/1.1 accepted",
            ));
        }
        let data = src.split_to(amt).freeze();
        let mut ret = Request::builder();
        ret = ret.method(method);
        let s = data.slice(path.0..path.1);
        let s = unsafe { String::from_utf8_unchecked(Vec::from(s.as_ref())) };
        ret = ret.uri(s);
        ret = ret.version(http::Version::HTTP_11);
        for header in headers.iter() {
            let (k, v) = match *header {
                Some((ref k, ref v)) => (k, v),
                None => break,
            };
            let value = HeaderValue::from_bytes(data.slice(v.0..v.1).as_ref())
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "header decode error"))?;
            ret = ret.header(&data[k.0..k.1], value);
        }

        let req = ret
            .body(())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(Some(req))
    }
}}