pub mod status_code;

use std::time::SystemTime;

use status_code::CodeLookup;

#[cfg(feature = "async")]
use smol;

use httparse;
use status_code::STATUS_CODES;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
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

    fn handle(&mut self, mut stream: TcpStream) {
        self.say(
            format!(
                "[INFO]    New connection from {}",
                stream.peer_addr().unwrap()
            )
            .as_str(),
        );

        let buf_reader = BufReader::new(&mut stream);
        let request_text = buf_reader
            .lines()
            .map(|result| result.unwrap())
            .take_while(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .as_slice()
            .join("\r\n");

        let mut headers = [httparse::EMPTY_HEADER; 16];
        let mut req = httparse::Request::new(&mut headers);
        req.parse(request_text.as_bytes()).unwrap();

        let splitted_temp = request_text.split("\r\n\r\n").collect::<Vec<&str>>();

        let body = if splitted_temp.len() > 1 {
            splitted_temp[1].to_string()
        } else {
            "".to_string()
        };

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

        let form = if req.method.unwrap() == "POST" {
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

        if let Some(callback) = self.route_callbacks.get(path) {
            // call the callback
            response = match callback.clone().try_lock() {
                Ok(prepared_callback) => prepared_callback(parsed_request.clone()),
                Err(_) => {
                    self.say(
                        "[ERROR]   Failed to acquire lock on callback (probably because it has panicked), attempting to error out with 500.",
                    );
                    ("INTERNAL SERVER ERROR".to_string(), 500)
                }
            };
        } else {
            self.say("[ERROR]   No route found, attempting to error out with 404.");

            response = self.find_error_or(parsed_request.clone(), ("NOT FOUND".to_string(), 404));
        }

        self.say(
            format!(
                "{} Responded with status code {} {} to {}",
                if response.1.to_string().chars().nth(0).unwrap() == '2' {
                    "[SUCCESS]"
                } else {
                    "[WARNING]"
                },
                response.1,
                STATUS_CODES.lookup(response.1).unwrap(),
                stream.peer_addr().unwrap()
            )
            .as_str(),
        );

        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n\r\n{}",
            response.1,
            STATUS_CODES.lookup(response.1).unwrap(),
            response.0.len(),
            response.0
        );

        stream.write_all(response.as_bytes()).unwrap();

        stream.flush().unwrap();

        stream.shutdown(Shutdown::Both).unwrap();

        return;
    }

    pub fn route<F>(&mut self, path: &str, callback: F)
    where
        F: Fn(ParsedRequest) -> (String, u16) + Send + 'static,
    {
        self.route_callbacks
            .insert(path.to_string(), Arc::new(Mutex::new(callback)));
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

        server.route("/", |req| {
            (
                format!(
                    "Hello, {}!",
                    req.args.get("name").unwrap_or(&"world".to_string())
                ),
                200,
            )
        });

        server.error(404, |_| ("Custom 404 page".to_string(), 404));

        server.run();
    }
}
