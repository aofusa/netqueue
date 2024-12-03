NetQueue
=====


以下の機能を持つネットワークキュー
- 1024バイト以下のバイト列のキューイング


使い方
-----
```sh
# no tls
cargo run

# with tls
cargo run --features tls
```

サーバ証明書の作成
```sh
openssl req -new -newkey ed25519 -nodes -text -out server.csr -keyout server.key -subj /CN=localhost
openssl x509 -req -in server.csr -text -days 3650 -extfile /dev/stdin -extensions v3_ca -signkey server.key -out server.crt <<EOF
[ v3_ca ]
subjectAltName = DNS:localhost, DNS:*.local
EOF
```

別タブでtelnetを実行
```sh
# TLSなしの場合
telnet localhost 5555

# TLSありの場合
openssl s_client -connect localhost:5555
```

コマンド
- pub <tag\>

  <tag\>キューに任意のデータを送れる状態にする

- sub <tag\>

  <tag\>キューにつなぎpubからデータ送信があれば表示する


例

別々のターミナルで開く
```sh
pub key
123123

sub key
123123
```

