use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::{Arc, Mutex};

#[cfg(not(feature = "tls"))]
use std::io::{Read, Write};

#[cfg(not(feature = "tls"))]
use std::net::{TcpListener, TcpStream};

#[cfg(not(feature = "tls"))]
use std::thread;

#[cfg(feature = "tls")]
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

#[cfg(feature = "tls")]
use rustls::ServerConfig;

#[cfg(feature = "tls")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(feature = "tls")]
use tokio::net::{TcpListener, TcpStream};

#[cfg(feature = "tls")]
use tokio_rustls::TlsStream;

#[cfg(not(feature = "tls"))]
fn handle_connection(mut stream: TcpStream, table: &Arc<Mutex<HashMap<String, VecDeque<Vec<u8>>>>>) -> Result<(), Box<dyn Error + '_>> {
    loop {
        let mut buffer = [0; 1024];
        stream.read(&mut buffer)?;
        println!("Request: {}", String::from_utf8_lossy(&buffer[..]));

        let msg_quit = b"quit";
        let msg_set = b"pub";
        let msg_get = b"sub";

        if buffer.starts_with(msg_quit) {
            let response = "QUIT\r\n";
            stream.write(response.as_bytes())?;
            stream.flush()?;
            break;
        } else if buffer.starts_with(msg_set) {
            let row = String::from_utf8_lossy(&buffer[..]);
            let data = row.split_whitespace().collect::<Vec<&str>>();
            let key = String::from(data[1]);
            let value = Vec::from(data[2..].join(" ").as_bytes());
            {
                let mut lock = table.lock()?;
                if lock.contains_key(&key) {
                    let queue = lock.get_mut(&key).unwrap();
                    queue.push_back(value);
                } else {
                    let mut queue = VecDeque::new();
                    queue.push_back(value);
                    lock.insert(key, queue);
                }
            }

            let response = "PUBLISHED\r\n";
            stream.write_all(response.as_bytes())?;
        } else if buffer.starts_with(msg_get) {
            let row = String::from_utf8_lossy(&buffer[..]);
            let data = row.split_whitespace().collect::<Vec<&str>>();
            let key = String::from(data[1]);
            let value = {
                let mut lock = table.lock()?;
                let queue = lock.get_mut(&key);
                match queue {
                    Some(q) => {
                        q.pop_front().unwrap_or_default()
                    },
                    None => vec![0; 0],
                }
            };
            stream.write_all(&*value)?;
            stream.write_all("\r\nSUBSCRIBE\r\n".as_bytes())?;
        }
    }

    Ok(())
}

#[cfg(not(feature = "tls"))]
fn main() -> Result<(), Box<dyn Error>> {
    let host = "127.0.0.1:5555";
    let listener = TcpListener::bind(host)?;
    println!("Server listen on {}", host);

    let table: Arc<Mutex<HashMap<String, VecDeque<Vec<u8>>>>> =
      Arc::new(Mutex::new(HashMap::new()));

    for stream in listener.incoming() {
        let stream = stream?;
        let t = table.clone();

        println!("Connection established!");
        thread::spawn(move || {
            handle_connection(stream, &t).unwrap();
        });
    }

    Ok(())
}

#[cfg(feature = "tls")]
async fn handle_connection(mut stream: TlsStream<TcpStream>, table: &Arc<Mutex<HashMap<String, VecDeque<Vec<u8>>>>>) -> Result<(), Box<dyn Error + '_>> {
    let msg_quit = b"quit";
    let msg_set = b"pub";
    let msg_get = b"sub";

    loop {
        let mut buffer = [0; 1024];
        match stream.read(&mut buffer).await {
            Ok(n) => {
                println!("Request: {}", String::from_utf8_lossy(&buffer[..n]));


                if buffer[..n].starts_with(msg_quit) {
                    let response = "QUIT\r\n";
                    stream.write_all(response.as_bytes()).await?;
                    break;
                } else if buffer.starts_with(msg_set) {
                    let row = String::from_utf8_lossy(&buffer[..n]);
                    let data = row.split_whitespace().collect::<Vec<&str>>();
                    let key = String::from(data[1]);
                    let value = Vec::from(data[2..].join(" ").as_bytes());
                    {
                        let mut lock = table.lock()?;
                        if lock.contains_key(&key) {
                            let queue = lock.get_mut(&key).unwrap();
                            queue.push_back(value);
                        } else {
                            let mut queue = VecDeque::new();
                            queue.push_back(value);
                            lock.insert(key, queue);
                        }
                    }

                    let response = "PUBLISHED\r\n";
                    stream.write_all(response.as_bytes()).await?;
                } else if buffer.starts_with(msg_get) {
                    let row = String::from_utf8_lossy(&buffer[..n]);
                    let data = row.split_whitespace().collect::<Vec<&str>>();
                    let key = String::from(data[1]);
                    let value = {
                        let mut lock = table.lock()?;
                        let queue = lock.get_mut(&key);
                        match queue {
                            Some(q) => {
                                q.pop_front().unwrap_or_default()
                            },
                            None => vec![0; 0],
                        }
                    };
                    stream.write_all(&*value).await?;
                    stream.write_all("\r\nSUBSCRIBE\r\n".as_bytes()).await?;
                }
            }
            Err(e) => eprintln!("Failed to read from TLS socket: {}", e),
        }
    }

    Ok(())
}

#[cfg(feature = "tls")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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

    let table: Arc<Mutex<HashMap<String, VecDeque<Vec<u8>>>>> =
      Arc::new(Mutex::new(HashMap::new()));

    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:5555").await?;
    println!("TLS Server listening on localhost:5555");

    while let Ok((stream, addr)) = listener.accept().await {
        println!("Accepted connection from: {}", addr);
        let acceptor = acceptor.clone();
        let t = table.clone();

        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    println!("TLS connection established with: {}", addr);
                    handle_connection(TlsStream::Server(tls_stream), &t).await.unwrap();
                }
                Err(e) => eprintln!("TLS acceptance failed: {}", e),
            }
        });
    }

    Ok(())
}
