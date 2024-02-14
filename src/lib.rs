use async_std::net::TcpListener;
use async_std::net::TcpStream;
use futures::stream::StreamExt;

async fn handle_client(stream: TcpStream) {
    let (reader, writer) = &mut (&stream, &stream);
    println!("New connection from: {}", stream.peer_addr().unwrap());
}
pub struct Server{
    port: u16,
    host: String,
}

impl Server {
    pub fn new(port: u16, host: String) -> Self {
        Server {
            port,
            host,
        }
    }

    pub async fn run(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(format!("{}:{}", self.host, self.port)).await?;
        let mut incoming = listener.incoming();
        while let Some(stream) = incoming.next().await {
            let stream = stream?;
            async_std::task::spawn(handle_client(stream));
        }
        Ok(())
    }
}