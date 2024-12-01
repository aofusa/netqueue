use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, mpsc, Mutex};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, Shutdown};
use std::thread;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

fn handle_connection(
    mut stream: TcpStream,
    table: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    pub_tx: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
    sub_rx: Arc<Mutex<HashMap<String, Arc<Mutex<Receiver<Vec<u8>>>>>>>,
    sub_client_tx: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
)-> Result<(), Box<dyn Error + 'static>> {
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
            stream.shutdown(Shutdown::Both)?;
            break;
        } else if buffer.starts_with(msg_set) || buffer.starts_with(msg_get) {
            let row = String::from_utf8_lossy(&buffer[..]);
            let data = row.split_whitespace().collect::<Vec<&str>>();
            let key = String::from(data[1]);

            // テーブルの状態チェック
            {
                let mut table_lock = table.lock().unwrap();
                if !&table_lock.contains_key(&key) {
                    let (t_pub_tx, t_pub_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
                    let (t_sub_tx, t_sub_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
                    let (t_sub_client_tx, t_sub_client_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();

                    // receiverをループする
                    let handle = thread::spawn(move || {
                        loop {
                            if let Ok(data) = t_pub_rx.recv() {
                                // sub が接続されるまで待機
                                if let Ok(_) = t_sub_client_rx.recv() {
                                    // sub が接続されたら data をレスポンスする
                                    t_sub_tx.send(data).unwrap();
                                }
                            }
                        }
                    });

                    table_lock.insert(key.clone(), handle);
                    pub_tx.lock().unwrap().insert(key.clone(), t_pub_tx);
                    sub_rx.lock().unwrap().insert(key.clone(), Arc::new(Mutex::new(t_sub_rx)));
                    sub_client_tx.lock().unwrap().insert(key.clone(), t_sub_client_tx);
                }
            }

            if buffer.starts_with(msg_set) {
                let value = Vec::from(data[2..].join(" ").as_bytes());

                pub_tx.lock().unwrap().get_mut(&key).unwrap().send(value)?;

                let response = "PUBLISHED\r\n";
                stream.write(response.as_bytes())?;
                stream.flush()?;
            } else if buffer.starts_with(msg_get) {
                // sub receiverにwriteされるまで待機(一定時間でタイムアウト) タイムアウト・切断されたらクライアント側で再接続
                sub_client_tx.lock().unwrap().get_mut(&key).unwrap().send(vec![])?;
                let srx = sub_rx.lock().unwrap().get(&key).unwrap().clone();
                match srx.lock().unwrap().recv_timeout(Duration::from_millis(30000)) {
                    Ok(response) => {
                        stream.write(&response)?;
                        stream.write("\r\nSUBSCRIBE\r\n".as_bytes())?;
                    },
                    Err(_) => {
                        stream.write("\r\nTIMEOUT\r\n".as_bytes())?;
                    }
                }
                stream.flush()?;

                // 空になったroomを削除
                // sub_rx.lock()?.get_mut(&key).unwrap();
            }
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let host = "127.0.0.1:5555";
    let listener = TcpListener::bind(host)?;
    println!("Server listen on {}", host);

    let table: Arc<Mutex<HashMap<String, JoinHandle<()>>>> = Arc::new(Mutex::new(HashMap::new()));
    let pub_tx: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>> = Arc::new(Mutex::new(HashMap::new()));
    let sub_rx: Arc<Mutex<HashMap<String, Arc<Mutex<Receiver<Vec<u8>>>>>>> = Arc::new(Mutex::new(HashMap::new()));
    let sub_client_tx: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>> = Arc::new(Mutex::new(HashMap::new()));

    for stream in listener.incoming() {
        let stream = stream?;
        let table = table.clone();
        let pub_tx = pub_tx.clone();
        let sub_rx = sub_rx.clone();
        let sub_client_tx = sub_client_tx.clone();

        println!("Connection established!");
        thread::spawn(move || {
            handle_connection(stream, table, pub_tx, sub_rx, sub_client_tx).unwrap();
        });
    }

    Ok(())
}
