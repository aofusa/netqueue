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
- pub <tag\> <value\>

  <tag\>キューに<value\>を追加

- sub <tag\>

  <tag\>キューに値が入るまで待機。値があれば取得

- quit
  接続の終了


例
```sh
pub key 123
PUBLISH

sub key
123
SUBSCRIBE

quit
QUIT
```

