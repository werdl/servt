//! # A simple, easy to use, and fast web server library for Rust.
//! Designed to be easy, simple and fast, though powerful enough to handle most use cases.
//!
//! ## Features
//! - [x] Simple and easy to use
//! - [x] Multi-threaded (or async if you enable the async feature
//! - [x] Redirects
//! - [x] Custom error pages
//! - [x] Custom routes (with query string and form parsing)
//! - [x] 100-continue support
//! - [x] Custom status codes
//!
//!
//! ## A few notes
//! - The server by default uses multithreading, but you can enable the async feature to use async instead.
//! - The Date header is included by default, but you can disable the time feature to exclude it (and save a couple of dependencies). If you don't enable it, bear in mind that is against the HTTP/1.1 spec, but most clients don't care, and thus it is included as an option in order to keep the library as lightweight as possible if configured that way.
//! - At the moment, the server does not support keep-alive connections, but it is planned for the future (along with possible HTTP/2 support).
//! - Deliberately does not support Last-Modified, If-Modified-Since, etc., because it is not designed to serve static files, but rather functions which are not trackable in the same way.
//! ### Ensure your callbacks don't panic
//! - If a callback panics, the mutex is poisoned and thus the server will error out with a 500 Internal Server Error on any subsequent requests to the route - therefore, ensure your callbacks don't panic (or handle any errors and return a 500 error).
//! ### Supported path formats
//! - `/path`
//! - `/path/`
//! - `/path?query=string`
//! - `/path/?query=string`
//! - `site.com/path`
//! - `site.com/path/`
//! - `site.com/path?query=string`
//! - `site.com/path/?query=string`
//!
//! - Note that the Host header is mandated by the HTTP/1.1 spec, but if a request is sent with HTTP 1.0, the server will not enforce it (else it will error out with a 400 Bad Request).
//!
//! ## Examples
//! ### Basic 'Hello, World!' server
//! ```rust
//! use servt::{Server, ParsedRequest};
//!
//! let mut server = Server::new(8080, "localhost".to_string());
//! server.route("/", |req| ("Hello, World!".to_string(), 200));
//!
//! server.run();
//! ```
//!
//! ### Example good practice form handling
//! ```rust
//! use servt::{Server, ParsedRequest};
//!
//! let mut server = Server::new(8080, "localhost".to_string());
//! server.route("/", |req| match req.form {
//!    Some(form) => (format!("Hello, {}!", form.get("name").unwrap_or(&"World".to_string())), 200),
//!    None => ("Hello, World!".to_string(), 200),
//! });
//!
//! server.run();
//! ```
//! 
//! ### Custom error pages
//! ```rust
//! use servt::{Server, ParsedRequest};
//! 
//! let mut server = Server::new(8080, "localhost".to_string());
//! server.error(404, |_| ("Custom 404 page".to_string(), 404));
//! 
//! server.run();
//! ```

/// The status code module, containing a lookup table for status codes.
pub mod status_code;

use std::time::SystemTime;

use status_code::CodeLookup;

#[cfg(feature = "time")]
use chrono::Utc;

use status_code::STATUS_CODES;
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/**
 * The ParsedRequest struct, containing the parsed request data.
 * Method, body and args are always present, while form is only present if the request is a POST request (and the body is form-urlencoded).
 * Can be cloned and compared, and is the exposed client-facing struct.
*/
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    pub method: String,
    pub body: String,
    pub args: HashMap<String, String>,
    pub form: Option<HashMap<String, String>>,
}

type Callback = dyn Fn(ParsedRequest) -> (String, u16) + Send + 'static;

/**
 * The Server struct, containing the server data.
 * The server is the main struct of the library, and is used to create and run the server.
 * The route_callbacks and error_callbacks are both HashMaps containing the path and the callback to be called when the path is accessed.
 * The redirects HashMap contains the paths to be redirected and the path to redirect to.
*/
#[derive(Clone)]
pub struct Server {
    port: u16,
    host: String,

    route_callbacks: HashMap<String, Arc<Mutex<Callback>>>,

    error_callbacks: HashMap<u16, Arc<Mutex<Callback>>>,

    redirects: HashMap<String, String>,

    pub should_continue: bool,

    started: u128,
}

impl Server {
    fn say(&self, s: &str) {
        println!(
            "[{}s] {}",
            ((SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                - self.started)
                / 1000),
            s
        );
    }

    /// Create a new server with the specified port and host. Can be configured with the route, error and redirect methods.
    pub fn new(port: u16, host: String) -> Self {
        Server {
            port,
            host,
            route_callbacks: HashMap::new(),
            error_callbacks: HashMap::new(),
            should_continue: true,
            started: 0,
            redirects: HashMap::new(),
        }
    }

    fn find_error_or(&self, request: ParsedRequest, default: (String, u16)) -> (String, u16) {
        let callback = &self.error_callbacks.get(&default.1);

        let (msg, code) = match callback {
            Some(callback) => callback.lock().unwrap()(request),
            None => default,
        };

        (msg, code)
    }

    fn search_for_headers(&self, headers: &[httparse::Header], key: &str) -> Option<String> {
        for header in headers {
            if header.name.to_lowercase() == key.to_lowercase() {
                return Some(String::from_utf8_lossy(header.value).to_string());
            }
        }

        None
    }

    fn error_with_code(&self, code: u16, request: ParsedRequest) -> (String, u16) {
        self.find_error_or(
            request,
            (STATUS_CODES.lookup(code).unwrap().to_string(), code),
        )
    }

    fn handle(&mut self, mut stream: TcpStream) {
        let mut code = 200;

        self.say(
            format!(
                "[INFO]    New connection from {}",
                stream.peer_addr().unwrap()
            )
            .as_str(),
        );

        let mut reader = BufReader::new(&stream);

        let mut request_text = String::new();

        loop {
            let mut buffer = [0; 1024];
            let bytes_read = reader.read(&mut buffer).unwrap();

            request_text.push_str(&String::from_utf8_lossy(&buffer[..bytes_read]));

            if bytes_read < 1024 {
                break;
            }
        }

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);

        if !req.headers.is_empty() {
            req.parse(request_text.as_bytes()).unwrap();
        }

        let body = if self
            .search_for_headers(req.headers, "Expect")
            .unwrap_or_default()
            == "100-continue"
            && (req.version.is_some() && req.version.unwrap() == 1)
        {
            self.say("[INFO]    Expect: 100-continue header found, sending 100 continue response");

            stream
                .write_all("HTTP/1.1 100 Continue\r\n\r\n".as_bytes())
                .unwrap();

            stream.flush().unwrap();

            // read the rest of the request to get the body
            let mut reader = BufReader::new(&stream);

            let mut body_text = String::new();

            loop {
                let mut buffer = [0; 1024];
                let bytes_read = reader.read(&mut buffer).unwrap();

                body_text.push_str(&String::from_utf8_lossy(&buffer[..bytes_read]));

                if bytes_read < 1024 {
                    break;
                }
            }

            body_text
        } else {
            let splitted_temp = request_text.split("\r\n\r\n").collect::<Vec<&str>>();

            if splitted_temp.len() > 1 {
                splitted_temp[1].to_string()
            } else {
                "".to_string()
            }
        };

        let required_headers = match req.version {
            Some(1) => vec!["Host"],
            _ => vec![],
        };

        for header in required_headers.iter() {
            if self.search_for_headers(req.headers, header).is_none() {
                self.say(
                    format!(
                        "[ERROR]   Missing required header: {}",
                        header.to_uppercase()
                    )
                    .as_str(),
                );
                code = 400;
            }
        }

        let mut args = HashMap::new();

        if req.path.unwrap_or_default().contains('?') {
            let mut iter = req.path.unwrap().split('?');
            let _ = iter.next().unwrap();
            let args_string = iter.next().unwrap();

            for pair in args_string.split('&') {
                let mut key_value = pair.split('=');
                let key = key_value.next().unwrap();
                let value = key_value.next().unwrap();
                args.insert(key.to_string(), value.to_string());
            }
        }

        let form = if req.method.unwrap_or_default() == "POST"
            && self
                .search_for_headers(req.headers, "Content-Type")
                .unwrap_or_default()
                .contains("application/x-www-form-urlencoded")
        {
            let mut form = HashMap::new();
            for pair in body.split('&') {
                let mut key_value = pair.split('=');

                if key_value.clone().count() != 2 {
                    break;
                }

                let key = key_value.next().unwrap();
                let value = key_value.next().unwrap();
                form.insert(key.to_string(), value.to_string());
            }

            if form.is_empty() {
                None
            } else {
                Some(form)
            }
        } else {
            None
        };

        let parsed_request = ParsedRequest {
            method: req.method.unwrap().to_string(),
            body: body.to_string(),
            args,
            form,
        };

        let path = req.path.unwrap().split('?').collect::<Vec<&str>>()[0];

        let response: (String, u16);

        let query_string = req
            .path
            .unwrap_or_default()
            .split('?')
            .collect::<Vec<&str>>();

        let query_string = query_string.get(1).unwrap_or(&"");

        // redirect raw path + query string
        let redirect_path = self.redirects.get(path);

        if let Some(redirect) = redirect_path {
            self.say(
                format!("[INFO]    Redirecting client from {} to {}", path, redirect).as_str(),
            );

            response = ("MOVED PERMENANTLY".to_string(), 301);
        } else if code != 200 {
            response = self.error_with_code(code, parsed_request.clone());
        } else {
            // paths can come in multiple formats, so we need to check for all of them
            // they are all taken to /path eventually
            // allowed types:
            // /path
            // /path/
            // /path?query=string
            // /path/?query=string
            // site.com/path

            let path = req.path.unwrap().trim_end_matches('/');

            // Remove leading slash if present
            let path = path.strip_prefix('/').unwrap_or(path);

            // Remove query string if present
            let path = path.split('?').next().unwrap();

            // Add leading slash
            let path = format!("/{}", path);

            if let Some(callback) = self.route_callbacks.get(&path) {
                // call the callback
                response = match callback.clone().try_lock() {
                    Ok(prepared_callback) => prepared_callback(parsed_request.clone()),
                    Err(_) => {
                        self.say(
                            "[ERROR]   Failed to acquire lock on callback (probably because it has panicked), attempting to error out with 500.",
                        );
                        self.error_with_code(500, parsed_request.clone())
                    }
                };
            } else {
                self.say("[ERROR]   No route found, attempting to error out with 404.");

                response = self.error_with_code(404, parsed_request.clone());
            }
        }

        self.say(
            format!(
                "{} Responded with status code {} {} to client on route {}",
                match response.1.to_string().chars().nth(0).unwrap() {
                    '1' => "[INFO]   ",
                    '2' => "[SUCCESS]",
                    '3' => "[INFO]   ",
                    '4' => "[ERROR]  ",
                    '5' => "[ERROR]  ",
                    _ => "[INFO]   ",
                },
                response.1,
                STATUS_CODES.lookup(response.1).unwrap(),
                req.path.unwrap()
            )
            .as_str(),
        );

        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}{}\r\nConnection: close{}\r\n\r\n{}",
            response.1,
            STATUS_CODES.lookup(response.1).unwrap(),
            response.0.len(),
            if cfg!(feature = "time") {
                // techincally we are not fully compliant without a Date header, but most clients don't care
                format!(
                    "\r\nDate: {}",
                    Utc::now().format("%a, %d %b %Y %T UTC").to_string()
                )
            } else {
                "".to_string()
            },
            if redirect_path.is_some() {
                format!(
                    "\r\nLocation: {}{}",
                    redirect_path.unwrap(),
                    if query_string.is_empty() {
                        "".to_string()
                    } else {
                        format!("?{}", query_string)
                    }
                )
            } else {
                "".to_string()
            },
            response.0
        );

        stream.write_all(response.as_bytes()).unwrap();

        stream.flush().unwrap();

        stream.shutdown(Shutdown::Both).unwrap();
    }

    /**
     * Add a route to the server.
     * The route is a path, and the callback is a function that takes a ParsedRequest and returns a tuple containing the response and the status code.
     */
    pub fn route<F>(&mut self, path: &str, callback: F)
    where
        F: Fn(ParsedRequest) -> (String, u16) + Send + 'static,
    {
        self.route_callbacks
            .insert(path.to_string(), Arc::new(Mutex::new(callback)));
    }

    /**
     * Add a redirect to the server.
     * The paths are the paths to redirect, and the to is the path to redirect to.
     */
    pub fn redirect(&mut self, paths: Vec<String>, to: String) {
        for path in paths {
            self.redirects.insert(path, to.clone());
        }
    }

    /**
     * Add an error route to the server.
     * The code is the status code, and the callback is a function that takes a ParsedRequest and returns a tuple containing the response and the status code.
     */
    pub fn error<F>(&mut self, code: u16, callback: F)
    where
        F: Fn(ParsedRequest) -> (String, u16) + Send + 'static,
    {
        self.error_callbacks
            .insert(code, Arc::new(Mutex::new(callback)));
    }

    /**
     * Run the server. This method will block the current thread and run the server, listening for incoming connections and handling them each in their own thread (or async task if you enable the async feature).
     */
    pub fn run(&mut self) {
        self.started = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let listener = TcpListener::bind(format!("{}:{}", self.host, self.port)).unwrap();
        println!("Server listening on {}:{}", self.host, self.port);

        for stream in listener.incoming() {
            if !self.should_continue {
                break;
            }

            let stream = stream.unwrap();

            let mut self_clone = self.clone();

            #[cfg(feature = "async")]
            smol::spawn(async move {
                self_clone.handle(stream);
            })
            .detach();

            #[cfg(not(feature = "async"))]
            std::thread::spawn(move || {
                self_clone.handle(stream);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server() {
        let mut server = Server::new(8080, "localhost".to_string());

        let mut server = Server::new(8080, "localhost".to_string());
        server.route("/", |req| match req.form {
            Some(form) => (
                format!("Hello, {}!", form.get("name").unwrap_or(&"World".to_string())),
                200,
            ),
            None => ("Hello, World!".to_string(), 200),
        });

        server.run();

        server.route("/echo", |req| (format!("{:?}", req), 200));

        server.redirect(vec!["/hello".to_string()], "/".to_string());

        server.error(404, |_| ("Custom 404 page".to_string(), 404));

        server.run();
    }
}
