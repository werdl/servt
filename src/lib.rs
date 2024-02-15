pub mod status_code;

use std::time::SystemTime;

use status_code::CodeLookup;

#[cfg(feature = "async")]
use smol;

use httparse;
use status_code::STATUS_CODES;
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    pub method: String,
    pub body: String,
    pub args: HashMap<String, String>,
    pub form: Option<HashMap<String, String>>,
}

#[derive(Clone)]
pub struct Server {
    port: u16,
    host: String,

    route_callbacks:
        HashMap<String, Arc<Mutex<dyn Fn(ParsedRequest) -> (String, u16) + Send + 'static>>>,

    error_callbacks:
        HashMap<u16, Arc<Mutex<dyn Fn(ParsedRequest) -> (String, u16) + Send + 'static>>>,

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
                / 1000) as u128,
            s
        );
    }
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

        let body: String;

        // check for 100-continue header
        if self
            .search_for_headers(&req.headers, "Expect")
            .unwrap_or_default()
            == "100-continue"
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

            body = body_text;
        } else {
            let splitted_temp = request_text.split("\r\n\r\n").collect::<Vec<&str>>();

            body = if splitted_temp.len() > 1 {
                splitted_temp[1].to_string()
            } else {
                "".to_string()
            };
        }

        let required_headers = ["Host"];

        for header in required_headers.iter() {
            if self.search_for_headers(&req.headers, header).is_none() {
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

        if req.path.unwrap_or_default().contains("?") {
            let mut iter = req.path.unwrap().split("?");
            let _ = iter.next().unwrap();
            let args_string = iter.next().unwrap();

            for pair in args_string.split("&") {
                let mut key_value = pair.split("=");
                let key = key_value.next().unwrap();
                let value = key_value.next().unwrap();
                args.insert(key.to_string(), value.to_string());
            }
        }

        let form = if req.method.unwrap_or_default() == "POST" {
            let mut form = HashMap::new();
            for pair in body.split("&") {
                let mut key_value = pair.split("=");

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

        let path = req.path.unwrap().split("?").collect::<Vec<&str>>()[0];

        let response: (String, u16);

        let query_string = req
            .path
            .unwrap_or_default()
            .split("?")
            .collect::<Vec<&str>>();

        let query_string = query_string.get(1).unwrap_or(&"");

        // redirect raw path + query string
        let redirect_path = self.redirects.get(path);

        if let Some(redirect) = redirect_path {
            self.say(
                format!("[INFO]    Redirecting client from {} to {}", path, redirect).as_str(),
            );

            response = ("MOVED PERMENANTLY".to_string(), 301);
        } else {
            if code != 200 {
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
                let path = if path.starts_with('/') {
                    &path[1..]
                } else {
                    path
                };

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
            "HTTP/1.1 {} {}\r\nContent-Length: {}{}\r\n\r\n{}",
            response.1,
            STATUS_CODES.lookup(response.1).unwrap(),
            response.0.len(),
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

    pub fn route<F>(&mut self, path: &str, callback: F)
    where
        F: Fn(ParsedRequest) -> (String, u16) + Send + 'static,
    {
        self.route_callbacks
            .insert(path.to_string(), Arc::new(Mutex::new(callback)));
    }

    pub fn redirect(&mut self, paths: Vec<String>, to: String) {
        for path in paths {
            self.redirects.insert(path, to.clone());
        }
    }

    pub fn error<F>(&mut self, code: u16, callback: F)
    where
        F: Fn(ParsedRequest) -> (String, u16) + Send + 'static,
    {
        self.error_callbacks
            .insert(code, Arc::new(Mutex::new(callback)));
    }

    pub fn run(&mut self) {
        self.started = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u128;
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

        server.route("/", |req| match req.form {
            Some(form) => (format!("Hello, {}!", form.get("name").unwrap()), 200),
            None => ("Hello, World!".to_string(), 200),
        });

        server.route("/echo", |req| (format!("{:?}", req), 200));

        server.redirect(vec!["/hello".to_string()], "/".to_string());

        server.error(404, |_| ("Custom 404 page".to_string(), 404));

        server.run();
    }
}
