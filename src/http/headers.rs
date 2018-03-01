extern crate httparse;

use std::io;
use std::str;
use std::collections::{HashMap, LinkedList};
use std::collections::hash_map::Entry;
use std::clone::Clone;

pub const DEFAULT_INTO_BUFFER_CAPACITY: usize = 65536;
pub const DEFAULT_HEADER_ROW_CAPACITY: usize = 256;

// \r\n, and ": "
const HEADER_EXTRA_BYTES: usize = 4;

const HEADER_SEPARATOR: &'static [u8] = b": ";
const HEADER_NEWLINE: &'static [u8] = b"\r\n";

#[derive(Debug)]
struct OctopusHeader {
    // Original header name with case intact. This is different to the keys in
    // the main header listing which are normalized.
    original_name: String,
    value: Vec<u8>,
    value_str: String,

    // Which header was this in the original request/response? 0 is first, 1 is
    // second, and so on.
    order: usize,

    // Length hint for this header.
    length_hint: usize,
}

impl OctopusHeader {
    pub fn new(original: String, contents: &Vec<u8>, order: usize) -> OctopusHeader {
        let length_hint = original.len() + contents.len() + HEADER_EXTRA_BYTES;
        OctopusHeader {
            original_name: original,
            value: contents.clone(),
            value_str: String::from_utf8(contents.clone()).unwrap(),
            order: order,
            length_hint: length_hint,
        }
    }

    pub fn value<'a>(&'a self) -> &'a Vec<u8> {
        &self.value
    }

    pub fn value_str<'a>(&'a self) -> &'a String {
        &self.value_str
    }

    pub fn original_name<'a>(&'a self) -> &'a String {
        &self.original_name
    }

    pub fn order(&self) -> usize {
        self.order
    }

    pub fn length_hint(&self) -> usize {
        self.length_hint
    }
}

impl Clone for OctopusHeader {
    fn clone(&self) -> Self {
        OctopusHeader {
            original_name: self.original_name().clone(),
            value: self.value().clone(),
            value_str: self.value_str().clone(),
            order: self.order,
            length_hint: self.length_hint,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.original_name = source.original_name().clone();
        self.value = source.value().clone();
        self.value_str = source.value_str().clone();
        self.order = source.order;
        self.length_hint = source.length_hint;
    }
}

#[derive(Debug)]
pub struct Headers {
    data: HashMap<String, LinkedList<OctopusHeader>>,
    total_count: usize,
}

impl<'a> Headers {
    pub fn new() -> Headers {
        Headers {
            data: HashMap::new(),
            total_count: 0,
        }
    }

    pub fn from_raw(raw: &[httparse::Header]) -> io::Result<Headers> {
        let mut headers = Headers::new();
        headers.total_count = raw.len();

        for header in raw {
            headers.insert(header.name, &(header.value.iter().cloned().collect()));
        }

        // Perform some basic verification.
        if headers.validate() {
            Ok(headers)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "header validation failed"))
        }
    }

    pub fn content_length(&self) -> Option<usize> {
        match self.get("content-length") {
            Some(value) => {
                Some(str::from_utf8(value).unwrap().parse().unwrap())
            },
            None => None
        }
    }

    pub fn get(&'a self, name: &str) -> Option<&'a Vec<u8>> {
        let name_lower = String::from(name).to_lowercase();
        match self.data.get(&name_lower) {
            Some(headers) => {
                match headers.front() {
                    Some(header) => Some(header.value()),
                    None => None,
                }
            },
            None => None
        }
    }

    pub fn insert(&mut self, name: &str, value: &Vec<u8>) {
        // Lowercase the header name for easier matching.
        let name_string = String::from(name);
        let mut item = match self.data.entry(name_string.to_lowercase()) {
            Entry::Occupied(entry) => {
                entry.into_mut()
            },
            Entry::Vacant(entry) => {
                entry.insert(LinkedList::new())
            },
        };

        item.push_back(OctopusHeader::new(name_string, value, self.total_count));
        self.total_count += 1;
    }

    fn validate(&self) -> bool {
        let host_ok = match self.data.get("host") {
            Some(list) => list.len() <= 1,
            None => true,
        };

        let length_ok = match self.data.get("content-length") {
            Some(list) => list.len() <= 1,
            None => true,
        };

        host_ok && length_ok
    }

    // Yields the UTF-8 version of the headers without a move.
    fn to_utf8(&self) -> Vec<u8> {
        // TODO: self.total_count and self.data should be protected by mutexes
        let empty_vec = Vec::<u8>::new();
        let mut temp = Vec::<Vec<u8>>::new();
        temp.resize(self.total_count + 1, empty_vec);

        // Put together the header rows and insert in the correct order.
        let mut bytes = 0;
        for (_, headers) in &self.data {
            for header in headers {
                let mut row = &mut temp[header.order()];
                row.reserve(header.length_hint());
                row.extend(header.original_name().as_bytes());
                row.extend(HEADER_SEPARATOR);
                row.extend(header.value());
                row.extend(HEADER_NEWLINE);

                bytes += row.len();
            }
        }

        // Always add a dummy row for the end-of-request newline
        temp[self.total_count] = HEADER_NEWLINE.iter().cloned().collect();
        bytes += HEADER_NEWLINE.len();

        // Collect rows into final Vec
        temp.into_iter().fold(Vec::with_capacity(bytes), |mut acc, v| {
            acc.extend(v); acc
        })
    }
}

impl Clone for Headers {
    fn clone(&self) -> Self {
        Headers {
            data: self.data.clone(),
            total_count: self.total_count,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.data = source.data.clone();
        self.total_count = source.total_count;
    }
}

impl Into<Vec<u8>> for Headers {
    fn into(self) -> Vec<u8> {
        self.to_utf8()
    }
}

#[cfg(test)]
mod tests {
    extern crate httparse;

    use super::*;
    use std::str;

    pub fn create_huge_headers() -> Headers {
        // Greatly exceed the default header capacity with demo headers.
        let mut headers = Headers::new();
        let test_value: Vec<u8> = "Test-Value".as_bytes().iter().cloned().collect();
        for _ in 0..DEFAULT_INTO_BUFFER_CAPACITY {
            headers.insert("Test-Header", &test_value);
        }

        headers
    }

    pub fn create_standard_headers() -> (Vec<u8>, Headers) {
        // Create a Headers object with a fairly standard set of headers.
        let headers_buf = b"Cache-Control: private, max-age=0\r\nContent-Encoding: gzip\r\nContent-Type: text/html; charset=UTF-8\r\nDate: Sat 28 Jan 2017 10:10:10 GMT\r\nExpires: -1\r\nServer: Foobar Server\r\nStrict-Transport-Security: max-age=86400\r\nX-XSS-Protection: 1; mode=block\r\nX-Frame-Options: SAMEORIGIN\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let (_, parsed) = httparse::parse_headers(headers_buf, &mut headers).unwrap().unwrap();

        (headers_buf.iter().cloned().collect(), Headers::from_raw(parsed).unwrap())
    }

    #[test]
    fn test_headers() {
        let mut headers = Headers::new();

        let value: Vec<u8> = "google.com".as_bytes().iter().cloned().collect();

        headers.insert("Host", &value);

        let host_result = headers.get("Host");
        assert!(host_result.is_some());
        assert_eq!(*host_result.unwrap(), value);
        assert_eq!(headers.get("Most"), None);
    }

    #[test]
    fn test_multiple_content_length() {
        let mut headers = Headers::new();

        let value1: Vec<u8> = "1234".as_bytes().iter().cloned().collect();
        let value2: Vec<u8> = "5678".as_bytes().iter().cloned().collect();

        headers.insert("Content-Length", &value1);
        headers.insert("Content-Length", &value2);

        assert_eq!(headers.content_length(), Some(1234));
    }

    #[test]
    fn test_good_parse() {
        let headers_buf = b"Host: foo.bar\r\nContent-Length: 10\r\nAccept: *\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 4];
        let (_, parsed) = httparse::parse_headers(headers_buf, &mut headers).unwrap().unwrap();

        let result = Headers::from_raw(parsed);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bad_parse_1() {
        let headers_buf = b"Host: foo.bar\r\nContent-Length: 10\r\nAccept: *\r\nHost: bar.baz\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 4];
        let (_, parsed) = httparse::parse_headers(headers_buf, &mut headers).unwrap().unwrap();

        let result = Headers::from_raw(parsed);
        assert!(result.is_err());
    }

    #[test]
    fn test_bad_parse_2() {
        let headers_buf = b"Host: foo.bar\r\nContent-Length: 10\r\nAccept: *\r\nContent-Length: 15\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 4];
        let (_, parsed) = httparse::parse_headers(headers_buf, &mut headers).unwrap().unwrap();

        let result = Headers::from_raw(parsed);
        assert!(result.is_err());
    }

    #[test]
    fn test_small_into() {
        let (source, headers) = create_standard_headers();

        // Perform comparison to ensure order and such is maintained. This uses
        // String instead of raw Vec<u8> comparison as Strings are significantly
        // easier to read in case of assertion failures.
        let buffer: Vec<u8> = headers.into();
        assert_eq!(String::from_utf8(buffer).unwrap(), String::from_utf8(source).unwrap());
    }

    #[test]
    fn test_massive_into() {
        let headers = create_huge_headers();

        let buffer: Vec<u8> = headers.into();
        assert!(buffer.len() > DEFAULT_INTO_BUFFER_CAPACITY);
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate httparse;

    extern crate test;
    use self::test::Bencher;

    use super::Headers;
    use super::tests;

    #[bench]
    fn standard_to_utf8_bench(b: &mut Bencher) {
        let (_, headers) = tests::create_standard_headers();
        b.iter(|| {
            test::black_box(headers.to_utf8())
        });
    }

    #[bench]
    fn huge_to_utf8_bench(b: &mut Bencher) {
        let headers = tests::create_huge_headers();
        b.iter(|| {
            test::black_box(headers.to_utf8())
        });
    }

    #[bench]
    fn parse_bench(b: &mut Bencher) {
        // TODO: work on centralizing this a bit
        let headers_buf = b"Cache-Control: private, max-age=0\r\nContent-Encoding: gzip\r\nContent-Type: text/html; charset=UTF-8\r\nDate: Sat 28 Jan 2017 10:10:10 GMT\r\nExpires: -1\r\nServer: Foobar Server\r\nStrict-Transport-Security: max-age=86400\r\nX-XSS-Protection: 1; mode=block\r\nX-Frame-Options: SAMEORIGIN\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let (_, parsed) = httparse::parse_headers(headers_buf, &mut headers).unwrap().unwrap();

        b.iter(|| {
            test::black_box(Headers::from_raw(parsed).unwrap())
        });
    }

    #[bench]
    fn successful_get(b: &mut Bencher) {
        let (_, headers) = tests::create_standard_headers();
        b.iter(|| {
            test::black_box(headers.get("host"))
        });
    }

    #[bench]
    fn unsuccessful_get(b: &mut Bencher) {
        let (_, headers) = tests::create_standard_headers();
        b.iter(|| {
            test::black_box(headers.get("most"))
        });
    }
}
