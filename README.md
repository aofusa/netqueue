NetQueue
=====


以下の機能を持つネットワークキュー
- 1024バイト以下のバイト列のキューイング


使い方
-----
```sh
cargo run
```

別タブでtelnetを実行
```sh
telnet localhost 5555
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

