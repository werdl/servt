use async_std::task;
use httparse;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

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
}

impl Server {
    pub fn new(port: u16, host: String) -> Self {
        Server {
            port,
            host,
            route_callbacks: HashMap::new(),
        }
    }

    fn handle(&self, mut stream: TcpStream) {
        let buf_reader = BufReader::new(&mut stream);
        let request_text = buf_reader
            .lines()
            .map(|result| result.unwrap())
            .take_while(|line| !line.is_empty())
            .collect::<Vec<_>>().as_slice().join("\r\n");


        let mut headers = [httparse::EMPTY_HEADER; 16];
        let mut req = httparse::Request::new(&mut headers);
        req.parse(request_text.as_bytes()).unwrap();

        println!("{:?}", request_text);


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

        println!("{:?}", parsed_request);

        if let Some(callback) = self.route_callbacks.get(req.path.unwrap()) {
            // call the callback
            let response = callback.lock().unwrap()(parsed_request);

            let response = format!(
                "HTTP/1.1 {}\r\nContent-Length: {}\r\n\r\n{}",
                response.1,
                response.0.len(),
                response.0
            );

            stream.write_all(response.as_bytes()).unwrap();
        } else {
            let response = "HTTP/1.1 404 NOT FOUND\r\nContent-Length: 0\r\n\r\n";
            stream.write_all(response.as_bytes()).unwrap();
        }

        println!("Request handled");

        stream.flush().unwrap();

        println!("Request handled");

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

    pub fn run(&self) {
        let listener = TcpListener::bind(format!("{}:{}", self.host, self.port)).unwrap();
        println!("Server listening on {}:{}", self.host, self.port);

        for stream in listener.incoming() {
            let stream = stream.unwrap();

            let self_clone = self.clone();
            task::spawn(async move {
                self_clone.handle(stream);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_server() {
        thread::spawn(move || {
            let mut server = Server::new(8080, "localhost".to_string());

            server.route("/", |req| {
                (
                    format!(
                        "Hello, you made a {} request with args {:?} and form {:?}",
                        req.method, req.args, req.form
                    ),
                    200,
                )
            });

            server.run();
        })
        .join()
        .unwrap();
    }
}
