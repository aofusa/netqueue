use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};

#[cfg(feature = "tls")]
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

#[cfg(feature = "tls")]
use rustls::ServerConfig;

#[cfg(feature = "tls")]
use tokio_rustls::TlsStream;

type Message = Vec<u8>;

struct MessageBox {
    subscriber: Arc<RwLock<Vec<Sender<Message>>>>,  // -> subへの配布
    publisher_tx: Sender<Message>,  // -> pubへ渡すためのSenderのコピー
    history: Arc<RwLock<VecDeque<Message>>>,  // pubの履歴
}

impl MessageBox {
    fn new(buffer_size: usize, max_history: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<Message>(buffer_size);
        let sub = Arc::new(RwLock::new(Vec::<Sender<Message>>::new()));
        let h = Arc::new(RwLock::new(VecDeque::with_capacity(max_history)));

        let mb = Self {
            subscriber: sub.clone(),
            publisher_tx: tx,
            history: h.clone(),
        };

        tokio::spawn(async move {
            let mut queue = Vec::new();

            loop {
                // publisherから受け取ってsubscriberに送信する
                while let Some(msg) = rx.recv().await {
                    if sub.read().unwrap().is_empty() {
                        // subscriberがいなければqueueに貯める
                        queue.push(msg.clone());
                    } else {
                        // queueを先に送信する
                        for q in queue.clone() {
                            let mut v = vec![];
                            if let Ok(reader) = sub.read() {
                                v = reader.to_vec().clone();
                            }
                            for s in v {
                                s.send(q.clone()).await.unwrap();
                            }
                        }
                        queue.clear();

                        // 現在のメッセージを各subscriberに送信する
                        let mut v = vec![];
                        if let Ok(reader) = sub.read() {
                            v = reader.to_vec().clone();
                        }
                        for s in v {
                            s.send(msg.clone()).await.unwrap();
                        }
                    }
                    // 履歴に追加する
                    if let Ok(mut writer) = h.write() {
                        writer.push_back(msg.clone());
                    }
                }
            }
        });

        mb
    }
}

struct Room {
    buffer_size: usize,
    message_box: MessageBox,  // -> connectionスレッドでpubされたものを各subに配布する
}

impl Room {
    fn new(buffer_size: usize, max_history: usize) -> Self {
        Self {
            buffer_size,
            message_box: MessageBox::new(buffer_size, max_history),
        }
    }

    fn join_publisher(&mut self) -> Sender<Message> {
        self.message_box.publisher_tx.clone()
    }

    fn join_subscriber(&mut self) -> Receiver<Message> {
        let (tx, rx) = mpsc::channel(self.buffer_size);
        if let Ok(mut writer) = self.message_box.subscriber.clone().write() {
            writer.push(tx);
        }
        rx
    }
}

struct RoomManager {
    buffer_size: usize,
    history_size: usize,
    table: HashMap<String, Arc<RwLock<Room>>>,
}

impl RoomManager {
    fn new(buffer_size: usize, history_size: usize) -> Self {
        Self {
            buffer_size,
            history_size,
            table: HashMap::new(),
        }
    }

    fn create(&mut self, tag: String) -> Arc<RwLock<Room>> {
        if self.table.contains_key(&tag) { self.table.get_mut(&tag).unwrap().clone() }
        else {
            let room = Arc::new(RwLock::new(Room::new(self.buffer_size, self.history_size)));
            self.table.insert(tag, room.clone());
            room
        }
    }
}

async fn handle_connection(
    #[cfg(feature = "tls")]
    mut stream: TlsStream<TcpStream>,
    #[cfg(not(feature = "tls"))]
    mut stream: TcpStream,
    room_manager: Arc<RwLock<RoomManager>>
) -> Result<(), Box<dyn Error + 'static>> {
    // pub <room>, sub <room> に分ける
    let msg_pub = b"pub";
    let msg_sub = b"sub";
    let mut tx = None;
    let mut rx = None;

    loop {
        let mut buffer = [0; 1024];
        match stream.read(&mut buffer).await {
            Ok(n) => {
                let raw = String::from_utf8_lossy(&buffer[..n]);
                println!("Request: {}", raw);

                if buffer[..n].starts_with(msg_pub) || buffer[..n].starts_with(msg_sub) {
                    let data = raw.split_whitespace().collect::<Vec<&str>>();
                    let tag = String::from(data[1]);

                    {
                        let room = room_manager.write().unwrap().create(tag);
                        if buffer[..n].starts_with(msg_pub) {
                            tx = Some(room.write().unwrap().join_publisher());
                        } else if buffer[..n].starts_with(msg_sub) {
                            rx = Some(room.write().unwrap().join_subscriber());
                        }
                    }

                    break;
                }
            },
            Err(e) => eprintln!("Failed to read from TLS socket: {}", e),
        }
    }

    if let Some(tx) = tx {
        // pub -> 送信し続ける
        loop {
            let mut buffer = [0; 1024];
            match stream.read(&mut buffer).await {
                Ok(n) => {
                    tx.send(buffer[..n].to_vec()).await?;
                },
                Err(e) => eprintln!("Failed to read from TLS socket: {}", e),
            }
        }
    }

    if let Some(mut rx) = rx {
        // sub -> 受け取り続ける
        loop {
            if let Some(msg) = rx.recv().await {
                stream.write_all(&*msg).await?;
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:5555").await?;
    let room_manager = Arc::new(RwLock::new(RoomManager::new(1024, 1024)));

    #[cfg(feature = "tls")]
    {
        println!("Loading certificate and key ...");

        let cert_file = std::fs::read("./credential/server.crt")?;
        let key_file = std::fs::read("./credential/server.key")?;

        let certs = rustls_pemfile::certs(&mut &*cert_file)
          .collect::<Result<Vec<_>, _>>()?
          .into_iter()
          .map(CertificateDer::from)
          .collect();

        println!("Certificate loaded successfully");

        let key = {
            let mut reader = &mut &*key_file;
            let mut private_keys = Vec::new();

            for item in rustls_pemfile::read_all(&mut reader) {
                match item {
                    Ok(rustls_pemfile::Item::Pkcs1Key(key)) => {
                        println!("Found PKCS1 key");
                        private_keys.push(PrivateKeyDer::Pkcs1(key));
                    }
                    Ok(rustls_pemfile::Item::Pkcs8Key(key)) => {
                        println!("Found PKCS8 key");
                        private_keys.push(PrivateKeyDer::Pkcs8(key));
                    }
                    Ok(_) => println!("Found other item"),
                    Err(e) => println!("Error reading key: {}", e),
                }
            }

            private_keys
              .into_iter()
              .next()
              .ok_or("no private key found")?
        };

        println!("Private key loaded successfully");

        let config = ServerConfig::builder()
          .with_no_client_auth()
          .with_single_cert(certs, key)?;

        println!("Server configuration created successfully");

        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        println!("TLS Server listening on localhost:5555");

        while let Ok((stream, addr)) = listener.accept().await {
            println!("Accepted connection from: {}", addr);
            let acceptor = acceptor.clone();
            let manager = room_manager.clone();

            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        println!("TLS connection established with: {}", addr);
                        handle_connection(TlsStream::Server(tls_stream), manager).await.unwrap();
                    }
                    Err(e) => eprintln!("TLS acceptance failed: {}", e),
                }
            });
        }
    }

    #[cfg(not(feature = "tls"))]
    {
        println!("Server listening on localhost:5555");

        while let Ok((stream, addr)) = listener.accept().await {
            println!("Accepted connection from: {}", addr);
            let manager = room_manager.clone();

            tokio::spawn(async move {
                println!("Connection established with: {}", addr);
                handle_connection(stream, manager).await.unwrap();
            });
        }
    }

    Ok(())
}
