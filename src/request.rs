extern crate httparse;
extern crate url;
extern crate mioco;

use std::io::prelude::*;
use std::str::FromStr;
use std::net;
use std::str;
use std::io;

use self::mioco::tcp::TcpStream;

use ::headers::Headers;

#[derive(Debug)]
pub struct Request<'buf> {
    pub url: url::Url,
    pub method: &'buf str,
    pub version: u8,
    pub headers: Headers<'buf>,
}

fn url_is_relative(url: &str) -> bool {
    let colon = url.find(':');
    let slash = url.find('/');

    match slash {
        Some(slash) => {
            // we have a slash!

            match colon {
                Some(colon) => {
                    slash < colon
                },
                None => {
                    // we don't have a colon, can't be absolute
                    true
                }
            }
        },
        None => {
            // no slash, can't be relative
            false
        }
    }
}

impl<'buf, 'headers> Request<'buf> {
    pub fn from_raw(request: httparse::Request<'buf, 'headers>) -> Request<'buf> {
        let path = request.path.unwrap();
        let mut url = Vec::new();
        let headers = Headers::from_raw(request.headers);

        if url_is_relative(path) {
            // FIXME: from the listening port, tell if it's secure or not for
            // the correct scheme.
            let secure = false;
            if secure {
                url.extend("https://".as_bytes());
            } else {
                url.extend("http://".as_bytes());
            }

            // FIXME: handle Host header missing
            url.extend(headers.get("Host").unwrap());
        }
        url.extend(path.as_bytes());

        Request {
            headers: headers,
            // FIXME: need to handle gluing the Host header to the URL if it's
            // relative.
            url: url::Url::parse(str::from_utf8(&url).unwrap()).unwrap(),
            method: request.method.unwrap(),
            version: request.version.unwrap(),
        }
    }

    // FIXME: Every method from here onwards should be moved onto traits or a
    // client library or something, not here.

    pub fn forward<S: Write>(&self, downstream: &mut S, body: Vec<u8>) {
        match self.connect() {
            Ok(mut upstream) => {
                upstream.write_all(&self.serialize()).unwrap();
                upstream.write_all(&body).unwrap();

                let mut buffer = [0; 65535];

                // FIXME: actually parse the response here.
                loop {
                    match upstream.read(&mut buffer) {
                        Ok(0) => {
                            break
                        },
                        Ok(n) => {
                            downstream.write_all(&buffer[..n]).unwrap();
                        },
                        Err(e) => {
                            println!("Error {}", e);
                            break
                        }
                    }
                }
            },
            Err(_) => {
                downstream.write_all(b"HTTP/1.1 501 Internal Server Error\r\nContent-Length: 6\r\nSorry\n").unwrap();
                return;
            }
        }
    }

    // FIXME: is there a serialization trait of some sort I can implement?
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::<u8>::with_capacity(65535);

        let reqline = format!("{} {} HTTP/1.{}\r\n", self.method, self.url.path(), self.version);
        out.extend(reqline.as_bytes());
        out.extend(self.headers.serialize());
        out.extend(b"\r\n");

        out
    }

    pub fn connect(&self) -> io::Result<TcpStream> {
        let domain = self.url.host_str().unwrap();
        let port = match self.url.port() {
            Some(port) => port,
            None => {
                match self.url.scheme() {
                    "http" => 80,
                    "https" => 443,
                    _ => panic!("Unknown scheme {}", self.url.scheme())
                }
            }
        };

        // FIXME: DNS Lookup. net::lookup_addrs is unstable and also blocking.
        let ip = net::IpAddr::from_str(domain).unwrap();
        let addr = net::SocketAddr::new(ip, port);

        // FIXME: configurable retry count
        for _ in 0..2 {
            match TcpStream::connect(&addr) {
                Ok(conn) => {
                    return Ok(conn);
                },
                Err(e) => {
                    println!("failed to connect: {}", e);
                }
            }
        }

        TcpStream::connect(&addr)
    }
}
